use std::{collections::{HashMap, HashSet, VecDeque}, convert::Infallible, env, net::{IpAddr, SocketAddr}, path::PathBuf, sync::Arc, time::{Duration, Instant}};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Multipart, Path, Query, State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    http::{header, request::Parts, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{sse::{Event, Sse}, IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Datelike, FixedOffset, Utc};
use futures_util::SinkExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, types::Json as SqlJson, FromRow, PgPool};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tokio::sync::{broadcast, Mutex};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    database: Option<PgPool>,
    upload_dir: PathBuf,
    cookie_secure: bool,
    external_http: reqwest::Client,
    discord_attachment_refresh: Option<DiscordAttachmentRefresh>,
    comment_push: Option<FlowboardCommentPush>,
    events: broadcast::Sender<()>,
    freeform_live: Arc<Mutex<HashMap<Uuid, FreeformLiveBoard>>>,
    board_presence: Arc<Mutex<HashMap<Uuid, HashMap<Uuid, BoardPresence>>>>,
    diagram_presence: Arc<Mutex<HashMap<Uuid, HashMap<Uuid, DiagramCursorPresence>>>>,
    diagram_locks: Arc<Mutex<HashMap<Uuid, HashMap<String, DiagramObjectLock>>>>,
    diagram_events: broadcast::Sender<DiagramLiveEvent>,
    freeform_live_events: broadcast::Sender<FreeformLiveEvent>,
    auth_rate_limiter: RateLimiter,
    trust_proxy: bool,
}

// Discord CDN URLs are short-lived. The Discord bridge owns the durable
// channel/message/attachment identifiers and exchanges them for a fresh URL.
#[derive(Clone)]
struct DiscordAttachmentRefresh {
    endpoint: reqwest::Url,
    signing_secret: String,
}

// A separate, narrow credential is used only for immediate Flowboard → Discord
// comment delivery. It must never be the board integration token.
#[derive(Clone)]
struct FlowboardCommentPush {
    endpoint: reqwest::Url,
    token: String,
    public_base_url: Option<reqwest::Url>,
}

#[derive(Serialize)]
struct DiscordAttachmentRefreshRequest<'a> {
    integration_id: Uuid,
    channel_id: &'a str,
    message_id: &'a str,
    attachment_id: &'a str,
}

#[derive(Deserialize)]
struct DiscordAttachmentRefreshResponse {
    url: String,
    #[serde(default, rename = "proxy_url")]
    _proxy_url: Option<String>,
}

#[derive(FromRow)]
struct AttachmentDownloadRecord {
    object_key: Option<String>,
    media_type: String,
    external_url: Option<String>,
    discord_channel_id: Option<String>,
    discord_message_id: Option<String>,
    discord_attachment_id: Option<String>,
    discord_integration_id: Option<Uuid>,
}

#[derive(Clone)]
struct RateLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    window: Duration,
    limit: usize,
    max_buckets: usize,
}

#[derive(Clone, Copy)]
struct CurrentUser {
    id: Uuid,
    session_id: Uuid,
}

#[derive(Clone, Copy)]
struct Viewer(Option<CurrentUser>);

#[derive(Clone, Copy)]
struct DiscordIntegration {
    id: Uuid,
    board_id: Uuid,
    default_list_id: Option<Uuid>,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
    database: &'static str,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
    message: String,
}

struct ApiError(StatusCode, &'static str, String);

impl ApiError {
    fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable", "PostgreSQL is not configured. Create .env from .env.example and start docker compose first.".to_owned())
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "validation_error", message.into())
    }

    fn internal(error: sqlx::Error) -> Self {
        tracing::error!(?error, "database query failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database operation could not be completed.".to_owned())
    }

    fn storage() -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, "storage_error", "Attachment storage operation could not be completed.".to_owned())
    }

    fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "authentication_required", "Sign in is required.".to_owned())
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self(StatusCode::FORBIDDEN, "access_denied", message.into())
    }

    fn too_many_requests(message: impl Into<String>) -> Self {
        Self(StatusCode::TOO_MANY_REQUESTS, "rate_limited", message.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorResponse { error: self.1, message: self.2 })).into_response()
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let session_token = cookie_value(&parts.headers, "flowboard_session").ok_or_else(ApiError::unauthorized)?;
        let csrf = if matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS) {
            None
        } else {
            Some(parts.headers.get("x-flowboard-csrf").and_then(|value| value.to_str().ok()).ok_or_else(|| ApiError::forbidden("A CSRF token is required."))?)
        };
        let pool = database(state)?;
        let session = if let Some(csrf) = csrf {
            sqlx::query_as::<_, (Uuid, Uuid)>(
                "SELECT s.user_id, s.id FROM sessions s INNER JOIN users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.csrf_token_hash = $2 AND s.revoked_at IS NULL AND s.expires_at > now() AND u.disabled_at IS NULL",
            )
            .bind(token_hash(&session_token))
            .bind(token_hash(csrf))
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
        } else {
            sqlx::query_as::<_, (Uuid, Uuid)>(
                "SELECT s.user_id, s.id FROM sessions s INNER JOIN users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.revoked_at IS NULL AND s.expires_at > now() AND u.disabled_at IS NULL",
            )
            .bind(token_hash(&session_token))
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?
        };
        if let Some((id, session_id)) = session {
            let _ = sqlx::query("UPDATE sessions SET last_seen_at = now() WHERE id = $1").bind(session_id).execute(pool).await;
            Ok(Self { id, session_id })
        } else {
            Err(ApiError::unauthorized())
        }
    }
}

impl FromRequestParts<AppState> for Viewer {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        match CurrentUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(Self(Some(user))),
            Err(error) if error.0 == StatusCode::UNAUTHORIZED => Ok(Self(None)),
            Err(error) => Err(error),
        }
    }
}

impl FromRequestParts<AppState> for DiscordIntegration {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let value = parts.headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok())
            .ok_or_else(ApiError::unauthorized)?;
        let token = value.strip_prefix("Bearer ").filter(|token| (48..=200).contains(&token.len()))
            .ok_or_else(ApiError::unauthorized)?;
        let integration = sqlx::query_as::<_, (Uuid, Uuid, Option<Uuid>)>(
            "SELECT id, board_id, default_list_id FROM discord_integrations WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(token_hash(token))
        .fetch_optional(database(state)?)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)?;
        let _ = sqlx::query("UPDATE discord_integrations SET last_used_at = now() WHERE id = $1")
            .bind(integration.0).execute(database(state)?).await;
        Ok(Self { id: integration.0, board_id: integration.1, default_list_id: integration.2 })
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Deserialize)]
struct CreateWorkspaceRequest {
    name: String,
}

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct UpdateProfileRequest { username: String }

#[derive(Deserialize)]
struct UpdateDiscordProfileRequest {
    #[serde(default)]
    discord_user_id: String,
}

#[derive(Serialize)]
struct DiscordProfileResponse {
    discord_user_id: Option<String>,
}

#[derive(Deserialize)]
struct ChangePasswordRequest { current_password: String, new_password: String }

#[derive(Deserialize)]
struct AcceptInvitationRequest {
    token: String,
    username: String,
    password: String,
}

#[derive(Serialize)]
struct AuthUserResponse {
    id: Uuid,
    username: String,
    avatar_url: Option<String>,
    is_system_owner: bool,
}

#[derive(Serialize)]
struct AuthResponse {
    user: AuthUserResponse,
}

#[derive(Serialize)]
struct AuthSetupResponse {
    registration_open: bool,
}

#[derive(FromRow)]
struct PasswordAccount {
    id: Uuid,
    username: String,
    password_hash: Option<String>,
    disabled_at: Option<DateTime<Utc>>,
    avatar_key: Option<String>,
    avatar_media_type: Option<String>,
    is_system_owner: bool,
}

#[derive(Serialize, FromRow)]
struct AccountInvitationResponse {
    id: Uuid,
    expires_at: String,
    #[sqlx(default)]
    token: Option<String>,
}

#[derive(Serialize)]
struct AccountInvitationPermissionResponse {
    can_create: bool,
}

#[derive(FromRow)]
struct AccountInvitationForAcceptance {
    id: Uuid,
}

#[derive(Serialize, FromRow)]
struct SessionResponse {
    id: Uuid,
    created_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    current: bool,
}

#[derive(Serialize)]
struct MemberPermissionsResponse {
    user_id: Uuid,
    permissions: Vec<String>,
}

#[derive(Deserialize)]
struct ReplaceMemberPermissionsRequest { permissions: Vec<String> }

#[derive(Clone, Serialize, FromRow)]
struct ProfileRoleResponse {
    id: Uuid,
    name: String,
    color: String,
    icon_shape: String,
    icon_color: String,
}

#[derive(Serialize)]
struct ProfileRoleCatalogResponse {
    roles: Vec<ProfileRoleResponse>,
    assigned_role_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct CreateProfileRoleRequest { name: String, color: String, icon_shape: String, icon_color: String }

#[derive(Deserialize)]
struct UpdateProfileRoleRequest { name: String, color: String, icon_shape: String, icon_color: String }

#[derive(Deserialize)]
struct ReplaceCardProfileRolesRequest { role_ids: Vec<Uuid> }

#[derive(Clone, Serialize, FromRow)]
struct DiagramResponse {
    id: Uuid,
    card_id: Uuid,
    title: String,
    document: Value,
    version: i32,
}

#[derive(Deserialize)]
struct ReplaceDiagramRequest {
    title: String,
    document: Value,
    version: Option<i32>,
}

#[derive(Deserialize)]
struct UpdateAccountStatusRequest { disabled: bool }

#[derive(Serialize, FromRow)]
struct AdminAccountResponse {
    id: Uuid,
    username: String,
    avatar_url: Option<String>,
    disabled_at: Option<DateTime<Utc>>,
    is_system_owner: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, FromRow)]
struct AdminWorkspaceResponse {
    id: Uuid,
    name: String,
    owner_username: String,
    member_count: i64,
    archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Serialize, FromRow)]
struct WorkspaceResponse {
    id: Uuid,
    name: String,
    background_image_url: Option<String>,
    can_manage: bool,
}

#[derive(Deserialize)]
struct CreateBoardRequest {
    title: String,
}

#[derive(Deserialize)]
struct UpdateBoardRequest {
    title: String,
}

#[derive(Deserialize)]
struct UpdateBoardBackgroundRequest {
    background_image_url: Option<String>,
    background_fit: Option<String>,
    background_position: Option<String>,
}

#[derive(Default)]
struct FreeformLiveBoard {
    cursors: HashMap<Uuid, FreeformCursorPresence>,
    pings: Vec<FreeformPingPresence>,
}

#[derive(Clone)]
struct BoardPresence {
    card_id: Option<Uuid>,
    editing_description: bool,
    location: BoardPresenceLocation,
    last_seen: Instant,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum BoardPresenceLocation {
    #[default]
    Board,
    Card,
    Diagram,
}

#[derive(Clone)]
struct DiagramCursorPresence { x: i32, y: i32, username: String, avatar_url: Option<String>, last_seen: Instant }

#[derive(Clone)]
struct DiagramObjectLock { user_id: Uuid, username: String, expires_at: Instant }

#[derive(Deserialize)]
struct UpdateDiagramPresenceRequest { x: i32, y: i32 }

#[derive(Serialize)]
struct DiagramPresenceEntry { user_id: Uuid, username: String, avatar_url: Option<String>, x: i32, y: i32 }

#[derive(Deserialize)]
struct DiagramMergeRequest {
    operation_id: Uuid,
    title: String,
    base_title: String,
    base_document: Value,
    document: Value,
}

#[derive(Clone, Serialize)]
struct DiagramMergeEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    card_id: Uuid,
    operation_id: Uuid,
    actor_id: Uuid,
    title: String,
    base_title: String,
    base_document: Value,
    document: Value,
}

#[derive(Clone, Serialize)]
struct DiagramCursorEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    card_id: Uuid,
    user_id: Uuid,
    username: String,
    avatar_url: Option<String>,
    x: i32,
    y: i32,
}

#[derive(Clone, Serialize)]
struct DiagramObjectLockEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    card_id: Uuid,
    object_id: String,
    user_id: Uuid,
    username: String,
    active: bool,
    expires_in_ms: u64,
}

#[derive(Clone, Serialize)]
struct DiagramNotesChangedEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    card_id: Uuid,
}

#[derive(Clone)]
enum DiagramLiveEvent {
    Merge(DiagramMergeEvent),
    Cursor(DiagramCursorEvent),
    ObjectLock(DiagramObjectLockEvent),
    NotesChanged(DiagramNotesChangedEvent),
}

#[derive(Deserialize)]
struct CreateDiagramNoteRequest { x: i32, y: i32, body: String }

#[derive(Deserialize)]
struct CreateDiagramNoteCommentRequest { body: String }

#[derive(FromRow)]
struct DiagramNoteRow {
    id: Uuid,
    x: i32,
    y: i32,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    created_at: String,
}

#[derive(FromRow)]
struct DiagramNoteCommentRow {
    id: Uuid,
    note_id: Uuid,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    body: String,
    created_at: String,
}

#[derive(Serialize)]
struct DiagramNoteCommentResponse {
    id: Uuid,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    body: String,
    created_at: String,
}

#[derive(Serialize)]
struct DiagramNoteResponse {
    id: Uuid,
    x: i32,
    y: i32,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    created_at: String,
    comments: Vec<DiagramNoteCommentResponse>,
}

#[derive(Deserialize)]
struct UpdateBoardPresenceRequest {
    #[serde(default)]
    card_id: Option<Uuid>,
    #[serde(default)]
    editing_description: bool,
    #[serde(default)]
    location: BoardPresenceLocation,
}

#[derive(Serialize)]
struct BoardPresenceEntry {
    user_id: Uuid,
    username: String,
    avatar_url: Option<String>,
    card_id: Option<Uuid>,
    card_title: Option<String>,
    editing_description: bool,
    location: BoardPresenceLocation,
}

#[derive(Clone)]
struct FreeformCursorPresence { x: i32, y: i32, last_seen: Instant }

#[derive(Clone)]
struct FreeformPingPresence { id: Uuid, user_id: Uuid, x: i32, y: i32, expires_at: Instant }

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FreeformLiveEvent {
    Cursor { board_id: Uuid, user_id: Uuid, username: String, avatar_url: Option<String>, x: i32, y: i32 },
    Ping { board_id: Uuid, id: Uuid, user_id: Uuid, username: String, avatar_url: Option<String>, x: i32, y: i32, expires_in_ms: u64 },
}

#[derive(Serialize)]
struct BoardBackgroundUploadResponse {
    url: String,
}

#[derive(Clone, Serialize, FromRow)]
struct BoardStickerRow {
    id: Uuid,
    name: String,
    media_type: String,
}

#[derive(Clone, Serialize)]
struct BoardStickerResponse {
    id: Uuid,
    name: String,
    media_type: String,
    url: String,
}

#[derive(Deserialize)]
struct UpdateBoardVisibilityRequest {
    visibility: String,
}

#[derive(Clone, Serialize, FromRow)]
struct BoardSummary {
    id: Uuid,
    title: String,
    visibility: String,
}

#[derive(Serialize)]
struct ImportBoardResponse {
    id: Uuid,
    title: String,
    imported_lists: usize,
    imported_cards: usize,
    imported_comments: usize,
}

#[derive(FromRow)]
struct BoardAccess {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    background_image_url: Option<String>,
    background_fit: String,
    background_position: String,
    visibility: String,
}

#[derive(Deserialize)]
struct CreateListRequest {
    title: String,
}

#[derive(Serialize, FromRow)]
struct BoardAutomationResponse {
    id: Uuid,
    name: String,
    list_id: Uuid,
    list_title: String,
    action_type: String,
    action_priority: Option<i16>,
    enabled: bool,
    created_at: String,
}

#[derive(Deserialize)]
struct CreateBoardAutomationRequest {
    name: String,
    list_id: Uuid,
    action_type: String,
    #[serde(default)]
    action_priority: Option<i16>,
}

#[derive(FromRow)]
struct BoardAutomationExecution {
    name: String,
    action_type: String,
    action_priority: Option<i16>,
}

#[derive(Deserialize)]
struct UpdateBoardAutomationRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct MoveListRequest {
    #[serde(default)]
    before_list_id: Option<Uuid>,
    #[serde(default)]
    below_list_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct UpdateBoardLayoutRequest {
    view_mode: String,
    #[serde(default)]
    positions: Vec<BoardFreeformPositionRequest>,
}

#[derive(Deserialize)]
struct BoardFreeformPositionRequest {
    list_id: Uuid,
    x: i32,
    y: i32,
}

#[derive(Serialize, FromRow)]
struct BoardFreeformPositionResponse {
    list_id: Uuid,
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct BoardLayoutResponse {
    view_mode: String,
    positions: Vec<BoardFreeformPositionResponse>,
}

#[derive(Deserialize)]
struct UpdateBoardFreeformCardPositionRequest {
    x: i32,
    y: i32,
}

#[derive(Serialize, FromRow)]
struct BoardFreeformCardPositionResponse {
    card_id: Uuid,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct UpdateFreeformLiveRequest {
    x: i32,
    y: i32,
    #[serde(default)]
    ping: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FreeformLiveSocketRequest {
    Cursor { x: i32, y: i32 },
    Ping { x: i32, y: i32 },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DiagramSocketRequest {
    Merge(DiagramMergeRequest),
    Live(DiagramLiveSocketRequest),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum DiagramLiveSocketRequest {
    Cursor { x: i32, y: i32 },
    ObjectLock { object_id: String, active: bool },
}

#[derive(Clone, Serialize, FromRow)]
struct FreeformLiveAccount {
    id: Uuid,
    username: String,
    avatar_url: Option<String>,
}

#[derive(Serialize)]
struct FreeformLiveCursorResponse {
    user_id: Uuid,
    username: String,
    avatar_url: Option<String>,
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct FreeformPingResponse {
    id: Uuid,
    user_id: Uuid,
    username: String,
    avatar_url: Option<String>,
    x: i32,
    y: i32,
    expires_in_ms: u64,
}

#[derive(Serialize)]
struct FreeformLiveResponse {
    cursors: Vec<FreeformLiveCursorResponse>,
    pings: Vec<FreeformPingResponse>,
}

#[derive(Serialize)]
struct BoardFreeformDrawingResponse { document: Value }

#[derive(Deserialize)]
struct ReplaceBoardFreeformDrawingRequest {
    document: Value,
    #[serde(default)]
    erase_foreign: bool,
}

#[derive(Serialize, FromRow)]
#[derive(Clone)]
struct ListResponse {
    id: Uuid,
    title: String,
    grid_column: i32,
    grid_row: i32,
    card_limit: i32,
    is_public: bool,
}

#[derive(Deserialize)]
struct CreateCardRequest {
    title: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct UpdateCardRequest {
    title: Option<String>,
    description: Option<String>,
    priority: Option<i16>,
    is_frozen: Option<bool>,
    #[serde(default)]
    start_at: Option<Option<String>>,
}

#[derive(Deserialize)]
struct CreateCardRelationRequest {
    target_card_id: Uuid,
    relation_type: String,
    #[serde(default)]
    note: String,
}

#[derive(Deserialize)]
struct UpdateCardRelationRequest {
    note: String,
}

#[derive(Deserialize)]
struct UpdateDependencyGraphNodePositionRequest {
    x: i32,
    y: i32,
}

#[derive(Serialize, FromRow)]
struct DependencyGraphNodePositionResponse {
    card_id: Uuid,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct UpdateCardPublicVisibilityRequest {
    is_public: bool,
}

#[derive(Deserialize)]
struct UpdateCardAccessThresholdsRequest {
    min_view_preset: String,
    min_edit_preset: String,
}

#[derive(Deserialize)]
struct UpdateListRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    card_limit: Option<i32>,
    #[serde(default)]
    is_public: Option<bool>,
}

#[derive(Deserialize)]
struct CreateChecklistItemRequest {
    title: String,
}

#[derive(Deserialize)]
struct CreateChecklistRequest {
    title: String,
}

#[derive(Deserialize)]
struct UpdateChecklistRequest {
    title: String,
}

#[derive(Deserialize)]
struct ReorderChecklistsRequest {
    checklist_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct UpdateChecklistItemRequest {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    is_completed: Option<bool>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct CreateCommentRequest {
    body: String,
    #[serde(default)]
    parent_comment_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct CreateDiscordIntegrationRequest {
    name: String,
    #[serde(default)]
    default_list_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct CreateDiscordCardRequest {
    source_id: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    list_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct CreateDiscordCommentRequest {
    message_id: String,
    author_name: String,
    #[serde(default)]
    author_avatar_url: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    attachments: Vec<DiscordAttachmentRequest>,
    #[serde(default, alias = "mentionedRoleIds")]
    mentioned_role_ids: Vec<Uuid>,
    #[serde(default, alias = "mentionedUserIds")]
    mentioned_user_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct ResolveDiscordUsersRequest {
    #[serde(alias = "discordUserIds")]
    discord_user_ids: Vec<String>,
}

#[derive(Serialize, FromRow)]
struct DiscordUserResolutionResponse {
    discord_user_id: String,
    user_id: Uuid,
    username: String,
}

#[derive(Deserialize)]
struct DiscordAttachmentRequest {
    url: String,
    filename: String,
    media_type: String,
    #[serde(default)]
    byte_size: i64,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    attachment_id: Option<String>,
}

// Flowboard keeps its own UUID for the rendered comment, while these fields
// identify the original Discord asset when its signed CDN URL expires.
struct DiscordAttachmentUpsert {
    id: Uuid,
    url: String,
    filename: String,
    media_type: String,
    byte_size: i64,
    channel_id: Option<String>,
    message_id: Option<String>,
    attachment_id: Option<String>,
}

#[derive(FromRow)]
struct CommentAttachmentReference {
    id: Uuid,
    discord_channel_id: Option<String>,
    discord_message_id: Option<String>,
    discord_attachment_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdateCommentRequest {
    body: String,
}

#[derive(Deserialize)]
struct ToggleCommentReactionRequest {
    emoji: String,
}

#[derive(Deserialize)]
struct UpdateDueDateRequest {
    due_at: String,
}

#[derive(Deserialize)]
struct UpdateCardCoverRequest {
    attachment_id: Option<Uuid>,
    #[serde(default = "default_cover_mode")]
    mode: String,
}

#[derive(Deserialize)]
struct UpdateCardBackgroundRequest {
    background_image_url: Option<String>,
}

fn default_cover_mode() -> String { "full".to_owned() }
fn default_role_shape() -> String { "circle".to_owned() }
fn default_icon_color() -> String { "#FFFFFF".to_owned() }

#[derive(Deserialize)]
struct UpdateCardCompletionRequest {
    is_completed: bool,
}

#[derive(Deserialize)]
struct UpdateCardWaitingRequest {
    #[serde(default)]
    user_id: Option<Uuid>,
    #[serde(default)]
    role_id: Option<Uuid>,
    note: String,
}

#[derive(Deserialize)]
struct UpdateCardPriorityRequest {
    priority: i16,
}

#[derive(Deserialize)]
struct CreateLabelRequest {
    name: String,
    color: String,
    #[serde(default = "default_role_shape")]
    icon_shape: String,
    #[serde(default = "default_icon_color")]
    icon_color: String,
}

#[derive(Deserialize)]
struct UpdateLabelRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    icon_shape: Option<String>,
    #[serde(default)]
    icon_color: Option<String>,
}

#[derive(Deserialize)]
struct UpdateDiscordLabelRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    icon_shape: Option<String>,
    #[serde(default)]
    icon_color: Option<String>,
}

#[derive(Deserialize)]
struct ReplaceDiscordCardLabelsRequest {
    label_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct ReplaceCardLabelsRequest {
    label_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct CreateMilestoneRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct UpdateMilestoneRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct ReplaceCardMilestoneRequest {
    #[serde(default)]
    milestone_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct AddExistingWorkspaceMemberRequest { user_id: Uuid }

#[derive(Deserialize)]
struct UpdateWorkspaceMemberRequest {
    preset: String,
}

#[derive(Deserialize)]
struct ReplaceCardAssigneesRequest {
    user_ids: Vec<Uuid>,
}

#[derive(Clone, Serialize, FromRow)]
struct AttachmentResponse {
    id: Uuid,
    original_name: String,
    media_type: String,
    byte_size: i64,
    url: String,
}

#[derive(Deserialize)]
struct MoveCardRequest {
    target_list_id: Uuid,
    #[serde(default)]
    before_card_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct DiscordCommentQuery {
    #[serde(default)]
    after: Option<Uuid>,
    #[serde(default)]
    limit: Option<u16>,
}

#[derive(Deserialize)]
struct SetDiscordCardCoverRequest {
    #[serde(default)]
    attachment_id: Option<Uuid>,
    #[serde(default)]
    attachment_url: Option<String>,
    #[serde(default = "default_cover_mode")]
    mode: String,
}

#[derive(Deserialize)]
struct MoveDiscordCardRequest {
    list_id: Uuid,
    #[serde(default)]
    before_card_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct BindDiscordThreadRequest {
    thread_id: String,
}

#[derive(Deserialize)]
struct DiscordCardSyncQuery {
    #[serde(default)]
    after: Option<i64>,
    #[serde(default)]
    limit: Option<u16>,
}

#[derive(Clone, Serialize, FromRow)]
struct CardResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    start_at: Option<String>,
}

#[derive(Serialize, FromRow)]
struct CardRelationResponse {
    id: Uuid,
    relation_type: String,
    note: String,
    direction: String,
    other_card_id: Uuid,
    other_card_title: String,
    other_card_list_id: Uuid,
    other_card_completed_at: Option<String>,
    created_at: String,
}

#[derive(Serialize, FromRow)]
struct BoardRelationResponse {
    id: Uuid,
    source_card_id: Uuid,
    target_card_id: Uuid,
    relation_type: String,
    note: String,
    created_at: String,
}

#[derive(Serialize, FromRow)]
struct CardReviewRow {
    status: String,
    updated_at: String,
    requested_by: Option<Uuid>,
    requested_by_name: Option<String>,
    requested_by_avatar_url: Option<String>,
}

#[derive(Serialize)]
struct CardReviewResponse {
    status: String,
    reviewers: Vec<MemberResponse>,
    decisions: Vec<CardReviewDecisionResponse>,
    requested_by: Option<MemberResponse>,
    updated_at: Option<String>,
}

#[derive(Serialize, FromRow)]
struct CardReviewDecisionResponse {
    reviewer_id: Uuid,
    #[serde(rename = "reviewer_username")]
    reviewer_name: String,
    reviewer_avatar_url: Option<String>,
    status: Option<String>,
    reason: Option<String>,
    decided_at: Option<String>,
}

#[derive(Deserialize)]
struct UpdateCardReviewRequest {
    status: String,
    #[serde(default)]
    reviewer_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
struct DecideCardReviewRequest {
    status: String,
    #[serde(default)]
    reason: String,
}

#[derive(Serialize, FromRow)]
struct MyTaskResponse {
    id: Uuid,
    board_id: Uuid,
    board_title: String,
    list_title: String,
    title: String,
    priority: i16,
    due_at: Option<String>,
    completed_at: Option<String>,
    updated_at: String,
    reasons: Vec<String>,
}

#[derive(Serialize, FromRow)]
struct CardDescriptionVersionResponse {
    id: Uuid,
    description: String,
    author_name: String,
    created_at: String,
}

#[derive(Serialize)]
struct CardPollOptionResponse {
    id: Uuid,
    title: String,
    votes: i64,
    voted: bool,
}

#[derive(Serialize)]
struct CardPollResponse {
    id: Uuid,
    question: String,
    created_by: String,
    created_at: String,
    options: Vec<CardPollOptionResponse>,
}

#[derive(FromRow)]
struct CardPollRow {
    id: Uuid,
    question: String,
    created_by: String,
    created_at: String,
}

#[derive(FromRow)]
struct CardPollOptionRow {
    poll_id: Uuid,
    id: Uuid,
    title: String,
    votes: i64,
    voted: bool,
}

#[derive(Deserialize)]
struct CreateCardPollRequest {
    question: String,
    options: Vec<String>,
}

#[derive(Deserialize)]
struct VoteCardPollRequest { option_id: Uuid }

#[derive(Serialize, FromRow)]
struct DiscordCardListResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    is_completed: bool,
    completed_at: Option<String>,
}

#[derive(Serialize, FromRow)]
struct DiscordCardStatusResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    is_completed: bool,
    completed_at: Option<String>,
}

#[derive(Serialize, FromRow)]
struct DiscordThreadCardResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    is_archived: bool,
    archived_at: Option<String>,
    is_completed: bool,
    completed_at: Option<String>,
    thread_id: Option<String>,
}

#[derive(Serialize, FromRow)]
struct DiscordCardSyncEventResponse {
    event_id: i64,
    event_kind: String,
    created_at: String,
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    is_archived: bool,
    archived_at: Option<String>,
    is_completed: bool,
    completed_at: Option<String>,
    thread_id: String,
}

#[derive(Serialize, FromRow)]
struct DiscordIntegrationResponse {
    id: Uuid,
    name: String,
    default_list_id: Option<Uuid>,
    created_at: String,
    last_used_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Clone, Serialize)]
struct ArchivedCardResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    completed_at: Option<String>,
    archived_at: String,
    labels: Vec<LabelResponse>,
    roles: Vec<ProfileRoleResponse>,
    assignees: Vec<MemberResponse>,
}

#[derive(FromRow)]
struct ArchivedCardRow {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    completed_at: Option<String>,
    archived_at: String,
}

#[derive(Deserialize)]
struct ArchivedCardsQuery {
    cursor: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct ArchivedCardsPageResponse {
    items: Vec<ArchivedCardResponse>,
    next_cursor: Option<Uuid>,
}

#[derive(Clone, FromRow)]
struct BoardCardRow {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    is_frozen: bool,
    last_activity_at: Option<String>,
    is_public: bool,
    min_view_preset: String,
    min_edit_preset: String,
    can_edit: bool,
    background_image_url: Option<String>,
    start_at: Option<String>,
    due_at: Option<String>,
    cover_attachment_id: Option<Uuid>,
    cover_url: Option<String>,
    cover_media_type: Option<String>,
    cover_mode: String,
    completed_at: Option<String>,
    checklist_total: i64,
    checklist_completed: i64,
    comment_count: i64,
    attachment_count: i64,
    has_unread_mentions: bool,
    has_unread_comments: bool,
    has_unvoted_polls: bool,
    milestone_id: Option<Uuid>,
    milestone_name: Option<String>,
    milestone_description: Option<String>,
    milestone_color: Option<String>,
    milestone_target_date: Option<String>,
}

#[derive(Clone, Serialize, FromRow)]
struct LabelResponse {
    id: Uuid,
    name: String,
    color: String,
    icon_shape: String,
    icon_color: String,
}

#[derive(Clone, Serialize, FromRow)]
struct MilestoneResponse {
    id: Uuid,
    name: String,
    description: String,
    color: String,
    target_date: Option<String>,
}

#[derive(FromRow)]
struct CardLabelRow {
    card_id: Uuid,
    id: Uuid,
    name: String,
    color: String,
    icon_shape: String,
    icon_color: String,
}

#[derive(FromRow)]
struct CardProfileRoleRow {
    card_id: Uuid,
    id: Uuid,
    name: String,
    color: String,
    icon_shape: String,
    icon_color: String,
}

#[derive(Clone, Serialize, FromRow)]
struct MemberResponse {
    id: Uuid,
    #[serde(rename = "username")]
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Clone, Serialize, FromRow)]
struct WorkspaceMemberManagementResponse {
    id: Uuid,
    username: String,
    preset: String,
    avatar_url: Option<String>,
}

#[derive(FromRow)]
struct CardAssigneeRow {
    card_id: Uuid,
    id: Uuid,
    display_name: String,
    avatar_url: Option<String>,
}

#[derive(Clone, Serialize, FromRow)]
struct CardWaitingResponse {
    card_id: Uuid,
    user_id: Option<Uuid>,
    user_name: Option<String>,
    role_id: Option<Uuid>,
    role_name: Option<String>,
    role_color: Option<String>,
    note: String,
}

#[derive(Clone, Serialize)]
struct BoardCard {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    priority: i16,
    is_frozen: bool,
    last_activity_at: Option<String>,
    is_public: bool,
    min_view_preset: String,
    min_edit_preset: String,
    can_edit: bool,
    background_image_url: Option<String>,
    start_at: Option<String>,
    due_at: Option<String>,
    cover_attachment_id: Option<Uuid>,
    cover_url: Option<String>,
    cover_media_type: Option<String>,
    cover_mode: String,
    completed_at: Option<String>,
    checklist_total: i64,
    checklist_completed: i64,
    comment_count: i64,
    attachment_count: i64,
    has_unread_mentions: bool,
    has_unread_comments: bool,
    has_unvoted_polls: bool,
    milestone: Option<MilestoneResponse>,
    waiting: Option<CardWaitingResponse>,
    labels: Vec<LabelResponse>,
    roles: Vec<ProfileRoleResponse>,
    assignees: Vec<MemberResponse>,
}

#[derive(Clone, Serialize, FromRow)]
struct ChecklistItemResponse {
    id: Uuid,
    title: String,
    is_completed: bool,
    description: String,
    attachments: Vec<AttachmentResponse>,
}

#[derive(Clone, Serialize)]
struct ChecklistResponse {
    id: Uuid,
    title: String,
    items: Vec<ChecklistItemResponse>,
}

#[derive(Serialize, FromRow)]
struct ChecklistRow {
    id: Uuid,
    title: String,
}

#[derive(FromRow)]
struct ChecklistItemRow {
    checklist_id: Uuid,
    id: Uuid,
    title: String,
    is_completed: bool,
    description: String,
}

#[derive(FromRow)]
struct ChecklistItemAttachmentRow {
    checklist_item_id: Uuid,
    id: Uuid,
    original_name: String,
    media_type: String,
    byte_size: i64,
    url: String,
}

#[derive(FromRow)]
struct ChecklistActivityRow {
    card_id: Uuid,
    title: String,
}

#[derive(FromRow)]
struct ChecklistItemActivityRow {
    id: Uuid,
    card_id: Uuid,
    title: String,
    is_completed: bool,
    description: String,
    checklist_title: String,
}

#[derive(Clone, Serialize)]
struct CommentResponse {
    id: Uuid,
    body: String,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    author_role_color: Option<String>,
    parent_comment_id: Option<Uuid>,
    created_at: String,
    edited_at: Option<String>,
    is_unread: bool,
    has_unread_thread: bool,
    reactions: Vec<CommentReactionResponse>,
    attachments: Vec<CommentAttachmentResponse>,
}

#[derive(Serialize)]
struct CommentThreadResponse {
    root: CommentResponse,
    comments: Vec<CommentResponse>,
}

#[derive(Clone, Serialize, FromRow)]
struct CommentAttachmentResponse {
    id: Uuid,
    original_name: String,
    media_type: String,
    byte_size: i64,
    download_url: String,
}

#[derive(FromRow)]
struct CommentRow {
    id: Uuid,
    body: String,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    author_role_color: Option<String>,
    parent_comment_id: Option<Uuid>,
    created_at: String,
    edited_at: Option<String>,
}

#[derive(Clone, Serialize)]
struct CommentReactionResponse {
    emoji: String,
    count: i64,
    reacted: bool,
}

#[derive(FromRow)]
struct CommentReactionRow {
    comment_id: Uuid,
    emoji: String,
    count: i64,
    reacted: bool,
}

#[derive(Clone, Serialize, FromRow)]
struct CardActivityResponse {
    id: Uuid,
    action: String,
    detail: String,
    actor_id: Option<Uuid>,
    actor_name: Option<String>,
    actor_avatar_url: Option<String>,
    actor_role_color: Option<String>,
    created_at: String,
}

#[derive(Serialize)]
struct CardDetail {
    checklists: Vec<ChecklistResponse>,
    comments: Vec<CommentResponse>,
    attachments: Vec<AttachmentResponse>,
    activity: Vec<CardActivityResponse>,
    cover_attachment_id: Option<Uuid>,
    cover_mode: String,
    background_image_url: Option<String>,
    unread_mention_source_ids: Vec<Uuid>,
    watching: bool,
}

#[derive(Deserialize)]
struct BoardActivityQuery {
    user_id: Option<Uuid>,
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Deserialize)]
struct BoardCardSearchQuery {
    q: String,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct BoardCardSearchResponse {
    card_ids: Vec<Uuid>,
}

#[derive(Serialize, FromRow)]
struct BoardActivityItemResponse {
    id: String,
    card_id: Uuid,
    card_title: String,
    action: String,
    detail: String,
    actor_id: Option<Uuid>,
    actor_name: String,
    actor_avatar_url: Option<String>,
    created_at: String,
    count: i64,
}

#[derive(Serialize)]
struct BoardActivityPageResponse {
    items: Vec<BoardActivityItemResponse>,
    page: i64,
    per_page: i64,
    total: i64,
}

#[derive(Serialize, FromRow)]
struct CardNotificationResponse {
    id: Uuid,
    card_id: Uuid,
    board_id: Uuid,
    card_title: String,
    board_title: String,
    actor_name: Option<String>,
    action: String,
    detail: String,
    is_read: bool,
    created_at: String,
    source_kind: Option<String>,
    source_id: Option<Uuid>,
}

#[derive(Serialize)]
struct CardWatchResponse {
    watching: bool,
}

#[derive(Serialize)]
struct BoardDetail {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    background_image_url: Option<String>,
    background_fit: String,
    background_position: String,
    visibility: String,
    can_edit: bool,
    can_admin: bool,
    labels: Vec<LabelResponse>,
    milestones: Vec<MilestoneResponse>,
    stickers: Vec<BoardStickerResponse>,
    members: Vec<MemberResponse>,
    lists: Vec<BoardList>,
}

#[derive(Serialize)]
struct BoardList {
    id: Uuid,
    title: String,
    grid_column: i32,
    grid_row: i32,
    card_limit: i32,
    is_public: bool,
    cards: Vec<BoardCard>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let upload_dir = PathBuf::from(env::var("FLOWBOARD_UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_owned()));
    tokio::fs::create_dir_all(&upload_dir).await.expect("could not create FLOWBOARD_UPLOAD_DIR");

    let cookie_secure = env::var("FLOWBOARD_COOKIE_SECURE").map(|value| value != "false").unwrap_or(false);
    let trust_proxy = env::var("FLOWBOARD_TRUST_PROXY").map(|value| value.eq_ignore_ascii_case("true")).unwrap_or(false);
    let frontend_origin = env::var("FLOWBOARD_API_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".to_owned());
    let database = match env::var("FLOWBOARD_DATABASE_URL") {
        Ok(url) => Some(connect_database(&url).await),
        Err(_) => {
            tracing::warn!("FLOWBOARD_DATABASE_URL is absent; data endpoints will return 503 until PostgreSQL is configured");
            None
        }
    };

    let cors = CorsLayer::new()
        .allow_origin(HeaderValue::try_from(frontend_origin).expect("FLOWBOARD_API_ORIGIN must be a valid HTTP origin"))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, HeaderName::from_static("x-flowboard-csrf")])
        .allow_credentials(true);
    let (events, _) = broadcast::channel(128);
    let external_http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("could not initialize external media client");
    let discord_attachment_refresh = discord_attachment_refresh_from_env();
    let comment_push = comment_push_from_env();

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/accept-invitation", post(accept_account_invitation))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/setup", get(auth_setup))
        // Unlike /me this endpoint is intentionally anonymous-safe. Public board links
        // use it to discover an optional signed-in account without generating a 401.
        .route("/v1/auth/state", get(auth_state))
        .route("/v1/auth/me", get(current_account).patch(update_profile))
        .route("/v1/auth/discord", get(get_discord_profile).patch(update_discord_profile))
        .route("/v1/auth/password", post(change_password))
        .route("/v1/auth/sessions", get(list_sessions).delete(revoke_other_sessions))
        .route("/v1/auth/sessions/{session_id}", axum::routing::delete(revoke_session))
        .route("/v1/auth/avatar", get(download_avatar).post(upload_avatar))
        .route("/v1/auth/account-invitations/permission", get(account_invitation_permission))
        .route("/v1/me/tasks", get(list_my_tasks))
        .route("/v1/profile-roles", get(list_profile_roles).post(create_profile_role))
        .route("/v1/profile-roles/{role_id}", patch(update_profile_role).delete(delete_profile_role))
        .route("/v1/profile-roles/self/{role_id}", put(assign_self_profile_role).delete(remove_self_profile_role))
        .route("/v1/avatars/{user_id}", get(download_user_avatar))
        .route("/v1/comments/{comment_id}/avatar", get(download_comment_avatar))
        .route("/v1/public/boards/{board_id}/background", get(download_public_board_background))
        .route("/v1/public/boards/{board_id}/avatars/{user_id}", get(download_public_board_avatar))
        .route("/v1/workspaces", get(list_workspaces).post(create_workspace))
        .route("/v1/workspaces/{workspace_id}", axum::routing::delete(delete_workspace))
        .route("/v1/workspaces/{workspace_id}/background", put(update_workspace_background))
        .route("/v1/workspaces/{workspace_id}/background/file", get(download_workspace_background).post(upload_workspace_background))
        .route("/v1/workspaces/{workspace_id}/members", get(list_workspace_members))
        .route("/v1/workspaces/{workspace_id}/available-accounts", get(list_available_workspace_accounts))
        .route("/v1/workspaces/{workspace_id}/members/existing", post(add_existing_workspace_member))
        .route("/v1/workspaces/{workspace_id}/settings/members", get(list_workspace_members_for_management))
        .route("/v1/workspaces/{workspace_id}/members/{user_id}", patch(update_workspace_member).delete(remove_workspace_member))
        .route("/v1/admin/accounts", get(list_accounts))
        .route("/v1/admin/accounts/{user_id}/status", patch(update_account_status))
        .route("/v1/admin/accounts/{user_id}", axum::routing::delete(delete_account))
        .route("/v1/admin/account-invitations", get(list_account_invitations).post(create_account_invitation))
        .route("/v1/admin/account-invitations/{invitation_id}", axum::routing::delete(revoke_account_invitation))
        .route("/v1/admin/workspaces", get(list_admin_workspaces))
        .route("/v1/admin/workspaces/{workspace_id}/archive", patch(archive_workspace))
        .route("/v1/admin/workspaces/{workspace_id}", axum::routing::delete(delete_workspace))
        .route("/v1/workspaces/{workspace_id}/members/{user_id}/permissions", get(list_member_permissions).put(replace_member_permissions))
        .route("/v1/workspaces/{workspace_id}/boards", get(list_boards).post(create_board))
        .route("/v1/workspaces/{workspace_id}/boards/import", post(import_trello_board))
        .route("/v1/boards/{board_id}/members", get(list_board_members_for_management))
        .route("/v1/boards/{board_id}/available-accounts", get(list_available_board_accounts))
        .route("/v1/boards/{board_id}/members/existing", post(add_existing_board_member))
        .route("/v1/boards/{board_id}/members/{user_id}", patch(update_board_member).delete(remove_board_member))
        .route("/v1/boards/{board_id}", get(get_board).patch(update_board).delete(delete_board))
        .route("/v1/boards/{board_id}/layout", get(get_board_layout).patch(update_board_layout))
        .route("/v1/boards/{board_id}/dependency-layout", get(get_dependency_graph_node_positions))
        .route("/v1/boards/{board_id}/dependency-layout/{card_id}", put(update_dependency_graph_node_position))
        .route("/v1/boards/{board_id}/freeform/cards", get(get_board_freeform_card_positions))
        .route("/v1/boards/{board_id}/freeform/cards/{card_id}", put(update_board_freeform_card_position).delete(clear_board_freeform_card_position))
        .route("/v1/boards/{board_id}/freeform/live", get(get_freeform_live).post(update_freeform_live))
        .route("/v1/boards/{board_id}/freeform/live/ws", get(freeform_live_websocket))
        .route("/v1/boards/{board_id}/presence", get(get_board_presence).put(update_board_presence).delete(leave_board_presence))
        .route("/v1/boards/{board_id}/freeform/drawing", get(get_board_freeform_drawing).put(replace_board_freeform_drawing))
        .route("/v1/boards/{board_id}/background", put(update_board_background))
        .route("/v1/boards/{board_id}/background/file", get(download_board_background).post(upload_board_background))
        .route("/v1/boards/{board_id}/stickers", get(list_board_stickers).post(upload_board_sticker))
        .route("/v1/boards/{board_id}/stickers/{sticker_id}", axum::routing::delete(delete_board_sticker))
        .route("/v1/boards/{board_id}/stickers/{sticker_id}/content", get(download_board_sticker))
        .route("/v1/boards/{board_id}/visibility", put(update_board_visibility))
        .route("/v1/boards/{board_id}/integrations/discord", get(list_discord_integrations).post(create_discord_integration))
        .route("/v1/boards/{board_id}/integrations/discord/{integration_id}", axum::routing::delete(revoke_discord_integration))
        .route("/v1/boards/{board_id}/export", get(export_board))
        .route("/v1/boards/{board_id}/archived-cards", get(list_archived_cards))
        .route("/v1/boards/{board_id}/search", get(search_board_cards))
        .route("/v1/boards/{board_id}/activity", get(list_board_activity))
        .route("/v1/boards/{board_id}/relations", get(list_board_relations))
        .route("/v1/boards/{board_id}/events", get(board_events))
        .route("/v1/boards/{board_id}/automations", get(list_board_automations).post(create_board_automation))
        .route("/v1/boards/{board_id}/automations/{automation_id}", patch(update_board_automation).delete(delete_board_automation))
        .route("/v1/boards/{board_id}/labels", post(create_label))
        .route("/v1/boards/{board_id}/milestones", post(create_milestone))
        .route("/v1/labels/{label_id}", patch(update_label).delete(delete_label))
        .route("/v1/milestones/{milestone_id}", patch(update_milestone).delete(delete_milestone))
        .route("/v1/boards/{board_id}/lists", post(create_list))
        .route("/v1/lists/{list_id}", patch(update_list).delete(delete_list))
        .route("/v1/lists/{list_id}/move", post(move_list))
        .route("/v1/lists/{list_id}/cards", post(create_card))
        .route("/v1/cards/{card_id}/move", post(move_card))
        .route("/v1/cards/{card_id}/restore", post(restore_card))
        .route("/v1/cards/{card_id}", axum::routing::patch(update_card).delete(archive_card))
        .route("/v1/cards/{card_id}/due-date", patch(update_due_date).delete(clear_due_date))
        .route("/v1/cards/{card_id}/labels", put(replace_card_labels))
        .route("/v1/cards/{card_id}/profile-roles", put(replace_card_profile_roles))
        .route("/v1/cards/{card_id}/milestone", put(replace_card_milestone))
        .route("/v1/cards/{card_id}/assignees", put(replace_card_assignees))
        .route("/v1/cards/{card_id}/cover", put(update_card_cover))
        .route("/v1/cards/{card_id}/background", put(update_card_background))
        .route("/v1/cards/{card_id}/background/file", get(download_card_background).post(upload_card_background))
        .route("/v1/cards/{card_id}/completion", patch(update_card_completion))
        .route("/v1/cards/{card_id}/waiting", put(update_card_waiting).delete(clear_card_waiting))
        .route("/v1/cards/{card_id}/review", get(get_card_review).put(update_card_review))
        .route("/v1/cards/{card_id}/review/decision", put(decide_card_review))
        .route("/v1/cards/{card_id}/relations", get(list_card_relations).post(create_card_relation))
        .route("/v1/cards/{card_id}/relations/{relation_id}", patch(update_card_relation).delete(delete_card_relation))
        .route("/v1/cards/{card_id}/description-versions", get(list_card_description_versions))
        .route("/v1/cards/{card_id}/description-versions/{version_id}/restore", post(restore_card_description_version))
        .route("/v1/cards/{card_id}/polls", get(list_card_polls).post(create_card_poll))
        .route("/v1/cards/{card_id}/polls/{poll_id}", axum::routing::delete(delete_card_poll))
        .route("/v1/cards/{card_id}/public-visibility", patch(update_card_public_visibility))
        .route("/v1/cards/{card_id}/access-thresholds", patch(update_card_access_thresholds))
        .route("/v1/polls/{poll_id}/vote", post(vote_card_poll))
        .route("/v1/cards/{card_id}/details", get(get_card_detail))
        .route("/v1/cards/{card_id}/watch", put(watch_card).delete(unwatch_card))
        .route("/v1/cards/{card_id}/mentions/read", post(mark_card_mentions_read))
        .route("/v1/notifications", get(list_notifications))
        .route("/v1/notifications/read", post(mark_all_notifications_read))
        .route("/v1/notifications/{notification_id}/read", post(mark_notification_read))
        .route("/v1/notifications/{notification_id}/unread", post(mark_notification_unread))
        .route("/v1/cards/{card_id}/diagram", get(get_card_diagram).put(replace_card_diagram))
        .route("/v1/cards/{card_id}/diagram/sync", post(sync_card_diagram))
        .route("/v1/cards/{card_id}/diagram/ws", get(card_diagram_websocket))
        .route("/v1/cards/{card_id}/diagram/presence", get(get_card_diagram_presence).put(update_card_diagram_presence))
        .route("/v1/cards/{card_id}/diagram/notes", get(list_card_diagram_notes).post(create_card_diagram_note))
        .route("/v1/diagram/notes/{note_id}", axum::routing::delete(delete_card_diagram_note))
        .route("/v1/diagram/notes/{note_id}/comments", post(create_card_diagram_note_comment))
        .route("/v1/cards/{card_id}/checklists", post(create_checklist))
        .route("/v1/cards/{card_id}/checklists/order", put(reorder_checklists))
        .route("/v1/checklists/{checklist_id}", patch(update_checklist).delete(delete_checklist))
        .route("/v1/checklists/{checklist_id}/items", post(create_checklist_item))
        .route("/v1/checklist-items/{item_id}", patch(update_checklist_item).delete(delete_checklist_item))
        .route("/v1/checklist-items/{item_id}/attachments", post(upload_checklist_item_attachment))
        .route("/v1/cards/{card_id}/comments", post(create_comment))
        .route("/v1/cards/{card_id}/comments/read", post(mark_card_comments_read))
        .route("/v1/integrations/discord/lists", get(list_discord_board_lists))
        .route("/v1/integrations/discord/roles", get(list_discord_profile_roles))
        .route("/v1/integrations/discord/users/resolve", post(resolve_discord_board_users))
        .route("/v1/discord-media/{token}/cards/{card_id}/avatars/{user_id}", get(download_discord_comment_avatar))
        .route("/v1/integrations/discord/labels", get(list_discord_labels).post(create_discord_label))
        .route("/v1/integrations/discord/labels/{label_id}", patch(update_discord_label).delete(delete_discord_label))
        .route("/v1/integrations/discord/cards/sync", get(list_discord_card_sync_events))
        .route("/v1/integrations/discord/threads/{thread_id}/card", get(get_discord_thread_card))
        .route("/v1/integrations/discord/cards", get(list_discord_board_cards).post(create_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}", get(get_discord_card).delete(archive_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}/restore", post(restore_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}/thread", put(bind_discord_card_thread))
        .route("/v1/integrations/discord/cards/{card_id}/labels", put(replace_discord_card_labels))
        .route("/v1/integrations/discord/cards/{card_id}/labels/{label_id}", post(add_discord_card_label).delete(remove_discord_card_label))
        .route("/v1/integrations/discord/cards/{card_id}/move", post(move_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}/cover", post(set_discord_card_cover))
        .route("/v1/integrations/discord/cards/{card_id}/completion", patch(set_discord_card_completion))
        .route("/v1/integrations/discord/cards/{card_id}/priority", patch(set_discord_card_priority))
        .route("/v1/integrations/discord/cards/{card_id}/comments", get(list_discord_card_comments).post(create_discord_comment))
        .route("/v1/integrations/discord/cards/{card_id}/attachments/{attachment_id}", get(download_discord_card_attachment))
        .route("/v1/comments/{comment_id}", patch(update_comment).delete(delete_comment))
        .route("/v1/comments/{comment_id}/thread", get(get_comment_thread))
        .route("/v1/comments/{comment_id}/thread/read", post(mark_comment_thread_read))
        .route("/v1/comments/{comment_id}/reactions", post(toggle_comment_reaction))
        .route("/v1/cards/{card_id}/attachments", post(upload_attachment))
        .route("/v1/attachments/{attachment_id}", get(download_attachment).delete(delete_attachment))
        .route("/v1/attachments/{attachment_id}/content", get(download_attachment))
        .with_state(AppState { database, upload_dir, cookie_secure, external_http, discord_attachment_refresh, comment_push, events, freeform_live: Arc::new(Mutex::new(HashMap::new())), board_presence: Arc::new(Mutex::new(HashMap::new())), diagram_presence: Arc::new(Mutex::new(HashMap::new())), diagram_locks: Arc::new(Mutex::new(HashMap::new())), diagram_events: broadcast::channel(1_024).0, freeform_live_events: broadcast::channel(256).0, auth_rate_limiter: RateLimiter::new(), trust_proxy })
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let bind_address = env::var("FLOWBOARD_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("FLOWBOARD_BIND_ADDR must be available");
    println!("Flowboard API is listening on http://{bind_address}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await.expect("API server failed");
}

async fn connect_database(url: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(url)
        .await
        .expect("could not connect to FLOWBOARD_DATABASE_URL");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("could not apply database migrations");
    println!("PostgreSQL migrations applied; persistent data is enabled");
    pool
}

fn database(state: &AppState) -> Result<&PgPool, ApiError> {
    state.database.as_ref().ok_or_else(ApiError::unavailable)
}

fn valid_text<'a>(value: &'a str, field: &'static str, max: usize) -> Result<&'a str, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::bad_request(format!("{field} must contain 1 to {max} characters.")));
    }
    Ok(value)
}

fn valid_due_at(value: &str) -> Result<DateTime<Utc>, ApiError> {
    let due_at: DateTime<FixedOffset> = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ApiError::bad_request("due_at must be an RFC 3339 timestamp with a timezone."))?;
    if !(1900..=2100).contains(&due_at.year()) {
        return Err(ApiError::bad_request("due_at must be between 1900 and 2100."));
    }
    Ok(due_at.with_timezone(&Utc))
}

fn valid_label_color(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.len() != 7 || !value.starts_with('#') || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::bad_request("color must be a hexadecimal value such as #6B7CFF."));
    }
    Ok(value.to_ascii_uppercase())
}

fn valid_username(value: &str) -> Result<String, ApiError> {
    let value = value.trim().to_ascii_lowercase();
    if !(3..=32).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')) {
        return Err(ApiError::bad_request("username must contain 3 to 32 lowercase letters, digits, dots, dashes, or underscores."));
    }
    Ok(value)
}

fn valid_discord_user_id(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if !(16..=22).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ApiError::bad_request("Discord User ID must be a numeric Discord snowflake."));
    }
    Ok(value.to_owned())
}

fn valid_password(value: &str) -> Result<&str, ApiError> {
    if value.chars().count() < 10 || value.chars().count() > 256 {
        return Err(ApiError::bad_request("password must contain 10 to 256 characters."));
    }
    Ok(value)
}

fn valid_discord_asset_url(value: &str) -> Result<&str, ApiError> {
    let value = value.trim();
    let host = value.strip_prefix("https://").and_then(|rest| rest.split('/').next()).unwrap_or_default();
    if value.len() > 2_000 || !matches!(host, "cdn.discordapp.com" | "media.discordapp.net" | "images-ext-1.discordapp.net" | "images-ext-2.discordapp.net") {
        return Err(ApiError::bad_request("Discord media URLs must use Discord's HTTPS CDN."));
    }
    Ok(value)
}

fn discord_media_markdown(name: &str, media_type: &str, url: &str) -> String {
    if media_type.starts_with("video/") { format!("![video:{name}]({url})") } else { format!("![{name}]({url})") }
}

fn token_hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

impl RateLimiter {
    fn new() -> Self {
        Self::with_limits(Duration::from_secs(10 * 60), 10, 4_096)
    }

    fn with_limits(window: Duration, limit: usize, max_buckets: usize) -> Self {
        Self { attempts: Arc::new(Mutex::new(HashMap::new())), window, limit, max_buckets }
    }

    async fn check(&self, action: &str, source: IpAddr) -> Result<(), ApiError> {
        self.check_at(action, source, Instant::now()).await
    }

    async fn check_at(&self, action: &str, source: IpAddr, now: Instant) -> Result<(), ApiError> {
        let mut attempts = self.attempts.lock().await;
        attempts.retain(|_, entries| {
            while entries.front().is_some_and(|time| now.saturating_duration_since(*time) > self.window) { entries.pop_front(); }
            !entries.is_empty()
        });
        let key = format!("{action}:{source}");
        if let Some(entries) = attempts.get_mut(&key) {
            if entries.len() >= self.limit { return Err(ApiError::too_many_requests("Too many attempts. Try again in a few minutes.")); }
            entries.push_back(now);
            return Ok(());
        }
        if attempts.len() >= self.max_buckets { return Err(ApiError::too_many_requests("Too many attempts. Try again in a few minutes.")); }
        attempts.insert(key, VecDeque::from([now]));
        Ok(())
    }
}

fn request_source_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(source) = headers.get("x-forwarded-for").and_then(|value| value.to_str().ok()).and_then(|value| value.split(',').next()).and_then(|value| value.trim().parse().ok()) {
            return source;
        }
    }
    peer.ip()
}

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers.get(header::COOKIE)?.to_str().ok()?.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}

fn session_cookies(jar: CookieJar, session_token: &str, csrf_token: &str, secure: bool) -> CookieJar {
    let session = Cookie::build(("flowboard_session", session_token.to_owned()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(secure)
        .build();
    let csrf = Cookie::build(("flowboard_csrf", csrf_token.to_owned()))
        .http_only(false)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(secure)
        .build();
    jar.add(session).add(csrf)
}

fn clear_session_cookies(jar: CookieJar, secure: bool) -> CookieJar {
    let mut session = Cookie::build(("flowboard_session", ""))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(secure)
        .build();
    session.make_removal();
    let mut csrf = Cookie::build(("flowboard_csrf", ""))
        .http_only(false)
        .same_site(SameSite::Strict)
        .path("/")
        .secure(secure)
        .build();
    csrf.make_removal();
    jar.add(session).add(csrf)
}

fn auth_response(account: PasswordAccount) -> AuthResponse {
    let _ = &account.avatar_media_type;
    AuthResponse { user: AuthUserResponse { id: account.id, username: account.username, avatar_url: account.avatar_key.map(|_| format!("/v1/avatars/{}", account.id)), is_system_owner: account.is_system_owner } }
}

async fn has_registered_account(pool: &PgPool) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE password_hash IS NOT NULL)")
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)
}

fn attachment_extension(media_type: &str, original_name: &str) -> Option<&'static str> {
    let from_media_type = match media_type {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/quicktime" => Some("mov"),
        "audio/webm" => Some("webm"),
        "audio/ogg" => Some("ogg"),
        "audio/mp4" => Some("m4a"),
        "audio/mpeg" => Some("mp3"),
        "audio/wav" => Some("wav"),
        _ => None,
    };
    from_media_type.or_else(|| match original_name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("jpg"), "png" => Some("png"), "gif" => Some("gif"), "webp" => Some("webp"), "mp4" => Some("mp4"), "webm" => Some("webm"), "mov" => Some("mov"), "ogg" => Some("ogg"), "m4a" => Some("m4a"), "mp3" => Some("mp3"), "wav" => Some("wav"), _ => None,
    })
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "flowboard-api",
        database: if state.database.is_some() { "ready" } else { "not_configured" },
    })
}

async fn register(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<RegisterRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    let username = valid_username(&request.username)?;
    let password = valid_password(&request.password)?;
    state.auth_rate_limiter.check("register", request_source_ip(&headers, peer, state.trust_proxy)).await?;
    let pool = database(&state)?;
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| ApiError::internal(sqlx::Error::Protocol("password hash failed".into())))?
        .to_string();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(6_372_084_913_i64)
        .execute(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    let registration_closed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE password_hash IS NOT NULL)")
        .fetch_one(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    if registration_closed {
        return Err(ApiError::forbidden("Registration is invite-only after the first workspace owner is created."));
    }
    let existing = sqlx::query_as::<_, PasswordAccount>(
        "SELECT id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner FROM users WHERE lower(username) = lower($1) FOR UPDATE",
    )
    .bind(&username)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?;
    let account = if let Some(existing) = existing {
        if existing.password_hash.is_some() {
            return Err(ApiError::bad_request("Этот ник уже занят."));
        }
        sqlx::query_as::<_, PasswordAccount>(
            "UPDATE users SET username = $1, display_name = $1, password_hash = $2, disabled_at = NULL, is_system_owner = TRUE WHERE id = $3 RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner",
        )
        .bind(username)
        .bind(password_hash)
        .bind(existing.id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ApiError::internal)?
    } else {
        sqlx::query_as::<_, PasswordAccount>(
            "INSERT INTO users (id, username, display_name, password_hash, is_system_owner) VALUES ($1, $2, $2, $3, TRUE) RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner",
        )
        .bind(Uuid::new_v4())
        .bind(username)
        .bind(password_hash)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ApiError::internal)?
    };
    transaction.commit().await.map_err(ApiError::internal)?;
    issue_session(&state, jar, account).await
}

async fn auth_setup(State(state): State<AppState>) -> ApiResult<AuthSetupResponse> {
    let registration_open = !has_registered_account(database(&state)?).await?;
    Ok(Json(AuthSetupResponse { registration_open }))
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<LoginRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    let username = valid_username(&request.username)?;
    state.auth_rate_limiter.check("login", request_source_ip(&headers, peer, state.trust_proxy)).await?;
    let account = sqlx::query_as::<_, PasswordAccount>(
        "SELECT id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner FROM users WHERE lower(username) = lower($1) LIMIT 1",
    )
    .bind(username)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::unauthorized())?;
    if account.disabled_at.is_some() || !account.password_hash.as_deref().is_some_and(|hash| PasswordHash::new(hash).ok().and_then(|parsed| Argon2::default().verify_password(request.password.as_bytes(), &parsed).ok()).is_some()) {
        return Err(ApiError::unauthorized());
    }
    issue_session(&state, jar, account).await
}

async fn accept_account_invitation(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    if request.token.len() < 32 || request.token.len() > 200 {
        return Err(ApiError::bad_request("Invitation token is invalid."));
    }
    state.auth_rate_limiter.check("invite", request_source_ip(&headers, peer, state.trust_proxy)).await?;
    let username = valid_username(&request.username)?;
    let password = valid_password(&request.password)?;
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let invitation = sqlx::query_as::<_, AccountInvitationForAcceptance>(
        "SELECT id FROM account_invitations WHERE token_hash = $1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at > now() FOR UPDATE",
    )
    .bind(token_hash(&request.token))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::bad_request("This invitation is no longer valid."))?;
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| ApiError::bad_request("Could not secure password."))?
        .to_string();
    let existing = sqlx::query_as::<_, PasswordAccount>("SELECT id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner FROM users WHERE lower(username) = lower($1) FOR UPDATE")
        .bind(&username).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?;
    let account = if let Some(existing) = existing {
        if existing.password_hash.is_some() { return Err(ApiError::bad_request("Этот ник уже занят.")); }
        sqlx::query_as::<_, PasswordAccount>("UPDATE users SET username = $1, display_name = $1, password_hash = $2, disabled_at = NULL WHERE id = $3 RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner")
            .bind(username).bind(password_hash).bind(existing.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    } else {
        sqlx::query_as::<_, PasswordAccount>("INSERT INTO users (id, username, display_name, password_hash) VALUES ($1, $2, $2, $3) RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner")
            .bind(Uuid::new_v4()).bind(username).bind(password_hash).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    };
    sqlx::query("UPDATE account_invitations SET accepted_at = now() WHERE id = $1")
        .bind(invitation.id)
        .execute(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    issue_session(&state, jar, account).await
}

async fn current_account(
    State(state): State<AppState>,
    current: CurrentUser,
) -> ApiResult<AuthResponse> {
    let account = sqlx::query_as::<_, PasswordAccount>(
        "SELECT id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner FROM users WHERE id = $1 AND disabled_at IS NULL",
    )
    .bind(current.id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(auth_response(account)))
}

fn is_discord_cdn_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && matches!(
        url.host_str(),
        Some("cdn.discordapp.com")
            | Some("media.discordapp.net")
            | Some("images-ext-1.discordapp.net")
            | Some("images-ext-2.discordapp.net")
    )
}

// Imported Trello exports reference the original attachment URL instead of a
// Flowboard upload.  Those files must pass through the same authorised media
// endpoint as Discord uploads so private boards do not expose attachment URLs
// directly.  Keep this deliberately narrow: import data is user supplied and
// this endpoint must never become an open HTTP proxy.
fn is_external_attachment_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https" && (is_discord_cdn_url(url) || matches!(
        url.host_str(),
        Some("trello.com")
            | Some("www.trello.com")
            | Some("api.trello.com")
            | Some("trello-attachments.s3.amazonaws.com")
            | Some("attachments.trello.services")
    ))
}

fn discord_attachment_refresh_from_env() -> Option<DiscordAttachmentRefresh> {
    let endpoint = env::var("FLOWBOARD_DISCORD_MEDIA_REFRESH_URL").ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    let signing_secret = env::var("FLOWBOARD_DISCORD_MEDIA_REFRESH_SIGNING_SECRET").ok().map(|value| value.trim().to_owned()).filter(|value| value.len() >= 32);
    let (Some(endpoint), Some(signing_secret)) = (endpoint, signing_secret) else {
        tracing::warn!("Discord attachment refresh is disabled: configure FLOWBOARD_DISCORD_MEDIA_REFRESH_URL and FLOWBOARD_DISCORD_MEDIA_REFRESH_SIGNING_SECRET");
        return None;
    };
    match reqwest::Url::parse(&endpoint) {
        Ok(endpoint) if endpoint.scheme() == "https" => Some(DiscordAttachmentRefresh { endpoint, signing_secret }),
        _ => {
            tracing::warn!("Discord attachment refresh is disabled: use an HTTPS URL and a signing secret of at least 32 characters");
            None
        }
    }
}

fn comment_push_from_env() -> Option<FlowboardCommentPush> {
    let token = env::var("FLOWBOARD_COMMENT_PUSH_TOKEN").ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| value.len() >= 24);
    let Some(token) = token else {
        tracing::warn!("Discord comment push is disabled: configure FLOWBOARD_COMMENT_PUSH_TOKEN");
        return None;
    };
    let endpoint = env::var("FLOWBOARD_COMMENT_PUSH_URL")
        .unwrap_or_else(|_| "https://yufu.su/api/flowboard/comments/push".to_owned());
    let public_base_url = env::var("FLOWBOARD_PUBLIC_URL")
        .or_else(|_| env::var("FLOWBOARD_API_ORIGIN"))
        .ok()
        .and_then(|value| reqwest::Url::parse(value.trim()).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    match reqwest::Url::parse(endpoint.trim()) {
        Ok(endpoint) if endpoint.scheme() == "https" => Some(FlowboardCommentPush { endpoint, token, public_base_url }),
        _ => {
            tracing::warn!("Discord comment push is disabled: FLOWBOARD_COMMENT_PUSH_URL must be an HTTPS URL");
            None
        }
    }
}

async fn auth_state(
    State(state): State<AppState>,
    viewer: Viewer,
) -> ApiResult<Option<AuthResponse>> {
    let Some(current) = viewer.0 else {
        return Ok(Json(None));
    };

    let Json(account) = current_account(State(state), current).await?;
    Ok(Json(Some(account)))
}

async fn update_profile(State(state): State<AppState>, current: CurrentUser, Json(request): Json<UpdateProfileRequest>) -> ApiResult<AuthResponse> {
    let username = valid_username(&request.username)?;
    let account = sqlx::query_as::<_, PasswordAccount>("UPDATE users SET username = $1, display_name = $1 WHERE id = $2 AND disabled_at IS NULL RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner")
        .bind(username).bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(auth_response(account)))
}

async fn get_discord_profile(State(state): State<AppState>, current: CurrentUser) -> ApiResult<DiscordProfileResponse> {
    let discord_user_id = sqlx::query_scalar::<_, String>("SELECT discord_user_id FROM user_discord_accounts WHERE user_id = $1")
        .bind(current.id)
        .fetch_optional(database(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(DiscordProfileResponse { discord_user_id }))
}

async fn update_discord_profile(State(state): State<AppState>, current: CurrentUser, Json(request): Json<UpdateDiscordProfileRequest>) -> ApiResult<DiscordProfileResponse> {
    let pool = database(&state)?;
    let value = request.discord_user_id.trim();
    if value.is_empty() {
        sqlx::query("DELETE FROM user_discord_accounts WHERE user_id = $1")
            .bind(current.id)
            .execute(pool)
            .await
            .map_err(ApiError::internal)?;
        return Ok(Json(DiscordProfileResponse { discord_user_id: None }));
    }
    let discord_user_id = valid_discord_user_id(value)?;
    let conflicting_account: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM user_discord_accounts WHERE discord_user_id = $1 AND user_id <> $2")
        .bind(&discord_user_id)
        .bind(current.id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?;
    if conflicting_account.is_some() {
        return Err(ApiError(StatusCode::CONFLICT, "discord_user_id_in_use", "This Discord account is already linked to another Flowboard profile.".to_owned()));
    }
    sqlx::query("INSERT INTO user_discord_accounts (user_id, discord_user_id) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET discord_user_id = EXCLUDED.discord_user_id, linked_at = now()")
        .bind(current.id)
        .bind(&discord_user_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(DiscordProfileResponse { discord_user_id: Some(discord_user_id) }))
}

async fn change_password(State(state): State<AppState>, current: CurrentUser, Json(request): Json<ChangePasswordRequest>) -> Result<StatusCode, ApiError> {
    let new_password = valid_password(&request.new_password)?;
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1 AND disabled_at IS NULL")
        .bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .flatten();
    let valid_current = hash.as_deref().is_some_and(|value| PasswordHash::new(value).ok().and_then(|parsed| Argon2::default().verify_password(request.current_password.as_bytes(), &parsed).ok()).is_some());
    if !valid_current { return Err(ApiError::forbidden("Current password is incorrect.")); }
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default().hash_password(new_password.as_bytes(), &salt).map_err(|_| ApiError::internal(sqlx::Error::Protocol("password hash failed".into())))?.to_string();
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2").bind(password_hash).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_avatar(State(state): State<AppState>, current: CurrentUser, mut multipart: Multipart) -> ApiResult<AuthResponse> {
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Avatar form is invalid."))?.ok_or_else(|| ApiError::bad_request("Avatar file is required."))?;
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_default();
    if !matches!(media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp") { return Err(ApiError::bad_request("Avatar must be JPEG, PNG, GIF, or WebP.")); }
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Avatar upload could not be read."))?;
    if bytes.is_empty() || bytes.len() > 5 * 1024 * 1024 { return Err(ApiError::bad_request("Avatar must be between 1 byte and 5 MiB.")); }
    let extension = attachment_extension(&media_type, "avatar").ok_or_else(|| ApiError::bad_request("Avatar format is invalid."))?;
    let key = format!("avatars/{}.{}", Uuid::new_v4(), extension);
    let path = state.upload_dir.join(&key);
    if let Some(parent) = path.parent() { tokio::fs::create_dir_all(parent).await.map_err(|_| ApiError::storage())?; }
    tokio::fs::write(&path, bytes).await.map_err(|_| ApiError::storage())?;
    let previous: Option<String> = sqlx::query_scalar("SELECT avatar_key FROM users WHERE id = $1")
        .bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?.flatten();
    sqlx::query("UPDATE users SET avatar_key = $1, avatar_media_type = $2 WHERE id = $3")
        .bind(&key).bind(&media_type).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if let Some(previous) = previous { let _ = tokio::fs::remove_file(state.upload_dir.join(previous)).await; }
    current_account(State(state), current).await
}

async fn avatar_response(state: &AppState, user_id: Uuid) -> Result<Response, ApiError> {
    let avatar = sqlx::query_as::<_, (String, String)>("SELECT avatar_key, avatar_media_type FROM users WHERE id = $1 AND disabled_at IS NULL AND avatar_key IS NOT NULL AND avatar_media_type IS NOT NULL")
        .bind(user_id).fetch_optional(database(state)?).await.map_err(ApiError::internal)?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(avatar.0)).await.map_err(|_| ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned()))?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&avatar.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("no-store"))], bytes).into_response())
}

async fn download_avatar(State(state): State<AppState>, current: CurrentUser) -> Result<Response, ApiError> {
    avatar_response(&state, current.id).await
}

async fn download_user_avatar(State(state): State<AppState>, _current: CurrentUser, Path(user_id): Path<Uuid>) -> Result<Response, ApiError> {
    avatar_response(&state, user_id).await
}

async fn download_comment_avatar(State(state): State<AppState>, current: Viewer, Path(comment_id): Path<Uuid>) -> Result<Response, ApiError> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
    .bind(comment_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned()))?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    let avatar_url = sqlx::query_scalar::<_, String>("SELECT external_author_avatar_url FROM comments WHERE id = $1 AND external_author_avatar_url IS NOT NULL")
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned()))?;
    proxy_external_attachment(&state.external_http, &avatar_url, "image/webp").await
}

async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    _current: CurrentUser,
) -> Result<CookieJar, ApiError> {
    if let Some(session_token) = jar.get("flowboard_session").map(|cookie| cookie.value().to_owned()) {
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
            .bind(token_hash(&session_token))
            .execute(database(&state)?)
            .await
            .map_err(ApiError::internal)?;
    }
    Ok(clear_session_cookies(jar, state.cookie_secure))
}

async fn list_sessions(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<SessionResponse>> {
    let sessions = sqlx::query_as::<_, (Uuid, DateTime<Utc>, DateTime<Utc>, DateTime<Utc>)>("SELECT id, created_at, last_seen_at, expires_at FROM sessions WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > now() ORDER BY last_seen_at DESC")
        .bind(current.id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?
        .into_iter().map(|(id, created_at, last_seen_at, expires_at)| SessionResponse { id, created_at, last_seen_at, expires_at, current: id == current.session_id }).collect();
    Ok(Json(sessions))
}

async fn revoke_session(State(state): State<AppState>, current: CurrentUser, Path(session_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    if session_id == current.session_id { return Err(ApiError::bad_request("Use sign out to revoke the current session.")); }
    let result = sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL")
        .bind(session_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::bad_request("Session is unavailable.")); }
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_other_sessions(State(state): State<AppState>, current: CurrentUser) -> Result<StatusCode, ApiError> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL")
        .bind(current.id).bind(current.session_id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn issue_session(
    state: &AppState,
    jar: CookieJar,
    account: PasswordAccount,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    let session_token = new_token();
    let csrf_token = new_token();
    sqlx::query("INSERT INTO sessions (id, user_id, token_hash, csrf_token_hash, expires_at) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(account.id)
        .bind(token_hash(&session_token))
        .bind(token_hash(&csrf_token))
        .bind(Utc::now() + chrono::Duration::days(30))
        .execute(database(state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok((session_cookies(jar, &session_token, &csrf_token, state.cookie_secure), Json(auth_response(account))))
}

async fn ensure_system_owner(pool: &PgPool, actor_id: Uuid) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_system_owner AND disabled_at IS NULL)")
        .bind(actor_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if allowed { Ok(()) } else { Err(ApiError::forbidden("System owner access is required.")) }
}

async fn can_create_account_invitations(pool: &PgPool, actor_id: Uuid) -> Result<bool, ApiError> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_system_owner AND disabled_at IS NULL)
         OR EXISTS(
             SELECT 1
             FROM workspace_members member
             INNER JOIN workspaces workspace ON workspace.id = member.workspace_id AND workspace.archived_at IS NULL
             INNER JOIN boards board ON board.workspace_id = workspace.id AND board.archived_at IS NULL
             WHERE member.user_id = $1 AND member.role IN ('owner', 'full_access')
         )",
    )
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)
}

async fn ensure_account_invitation_creator(pool: &PgPool, actor_id: Uuid) -> Result<(), ApiError> {
    if can_create_account_invitations(pool, actor_id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("Full Access on an active board is required to create account invitations."))
    }
}

fn valid_profile_role_shape(value: &str) -> Result<&str, ApiError> {
    match value {
        "circle" | "square" | "diamond" | "star" | "triangle" | "hexagon" | "bolt" | "flag" | "check" | "cross" => Ok(value),
        _ => Err(ApiError::bad_request("Unsupported role icon shape.")),
    }
}

async fn list_profile_roles(State(state): State<AppState>, current: CurrentUser) -> ApiResult<ProfileRoleCatalogResponse> {
    let pool = database(&state)?;
    let roles = sqlx::query_as::<_, ProfileRoleResponse>("SELECT id, name, color, icon_shape, icon_color FROM profile_roles ORDER BY name")
        .fetch_all(pool).await.map_err(ApiError::internal)?;
    let assigned_role_ids = sqlx::query_scalar::<_, Uuid>("SELECT role_id FROM user_profile_roles WHERE user_id = $1 ORDER BY role_id")
        .bind(current.id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(ProfileRoleCatalogResponse { roles, assigned_role_ids }))
}

async fn create_profile_role(State(state): State<AppState>, current: CurrentUser, Json(request): Json<CreateProfileRoleRequest>) -> ApiResult<ProfileRoleResponse> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    let name = valid_text(&request.name, "name", 80)?.to_owned();
    let color = valid_label_color(&request.color)?;
    let icon_shape = valid_profile_role_shape(&request.icon_shape)?.to_owned();
    let icon_color = valid_label_color(&request.icon_color)?;
    let role = sqlx::query_as::<_, ProfileRoleResponse>("INSERT INTO profile_roles (id, name, color, icon_shape, icon_color, created_by) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, name, color, icon_shape, icon_color")
        .bind(Uuid::new_v4()).bind(name).bind(color).bind(icon_shape).bind(icon_color).bind(current.id)
        .fetch_one(pool).await.map_err(ApiError::internal)?;
    record_audit(pool, current.id, None, None, "profile_role.created").await;
    Ok(Json(role))
}

async fn update_profile_role(State(state): State<AppState>, current: CurrentUser, Path(role_id): Path<Uuid>, Json(request): Json<UpdateProfileRoleRequest>) -> ApiResult<ProfileRoleResponse> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    let name = valid_text(&request.name, "name", 80)?.to_owned();
    let color = valid_label_color(&request.color)?;
    let icon_shape = valid_profile_role_shape(&request.icon_shape)?.to_owned();
    let icon_color = valid_label_color(&request.icon_color)?;
    let role = sqlx::query_as::<_, ProfileRoleResponse>("UPDATE profile_roles SET name = $1, color = $2, icon_shape = $3, icon_color = $4, updated_at = now() WHERE id = $5 RETURNING id, name, color, icon_shape, icon_color")
        .bind(name).bind(color).bind(icon_shape).bind(icon_color).bind(role_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "profile_role_not_found", "Role was not found.".to_owned()))?;
    record_audit(pool, current.id, None, None, "profile_role.updated").await;
    Ok(Json(role))
}

async fn delete_profile_role(State(state): State<AppState>, current: CurrentUser, Path(role_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    let result = sqlx::query("DELETE FROM profile_roles WHERE id = $1").bind(role_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "profile_role_not_found", "Role was not found.".to_owned())); }
    record_audit(pool, current.id, None, None, "profile_role.deleted").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_self_profile_role(State(state): State<AppState>, current: CurrentUser, Path(role_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM profile_roles WHERE id = $1)").bind(role_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !exists { return Err(ApiError(StatusCode::NOT_FOUND, "profile_role_not_found", "Role was not found.".to_owned())); }
    sqlx::query("INSERT INTO user_profile_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(current.id).bind(role_id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_self_profile_role(State(state): State<AppState>, current: CurrentUser, Path(role_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM user_profile_roles WHERE user_id = $1 AND role_id = $2").bind(current.id).bind(role_id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn record_audit(pool: &PgPool, actor_id: Uuid, workspace_id: Option<Uuid>, target_user_id: Option<Uuid>, action: &str) {
    if let Err(error) = sqlx::query("INSERT INTO audit_log (id, actor_id, workspace_id, target_user_id, action) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4()).bind(actor_id).bind(workspace_id).bind(target_user_id).bind(action)
        .execute(pool).await {
        tracing::warn!(?error, "audit log write failed");
    }
}

async fn list_accounts(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<AdminAccountResponse>> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    Ok(Json(sqlx::query_as("SELECT id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url, disabled_at, is_system_owner, created_at FROM users WHERE password_hash IS NOT NULL ORDER BY username")
        .fetch_all(pool).await.map_err(ApiError::internal)?))
}

async fn update_account_status(State(state): State<AppState>, current: CurrentUser, Path(user_id): Path<Uuid>, Json(request): Json<UpdateAccountStatusRequest>) -> ApiResult<AdminAccountResponse> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    if user_id == current.id { return Err(ApiError::bad_request("System owner cannot block their own account.")); }
    let account = sqlx::query_as("UPDATE users SET disabled_at = CASE WHEN $1 THEN now() ELSE NULL END WHERE id = $2 AND NOT is_system_owner RETURNING id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url, disabled_at, is_system_owner, created_at")
        .bind(request.disabled).bind(user_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Account is unavailable or protected."))?;
    if request.disabled {
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL").bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    }
    record_audit(pool, current.id, None, Some(user_id), if request.disabled { "account.blocked" } else { "account.unblocked" }).await;
    let _ = state.events.send(());
    Ok(Json(account))
}

async fn delete_account(State(state): State<AppState>, current: CurrentUser, Path(user_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    if user_id == current.id { return Err(ApiError::bad_request("System owner cannot delete their own account.")); }
    let result = sqlx::query("DELETE FROM users WHERE id = $1 AND NOT is_system_owner").bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::bad_request("Account is unavailable or protected.")); }
    record_audit(pool, current.id, None, Some(user_id), "account.deleted").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_account_invitations(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<AccountInvitationResponse>> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    Ok(Json(sqlx::query_as("SELECT id, expires_at::text AS expires_at, NULL::text AS token FROM account_invitations WHERE accepted_at IS NULL AND revoked_at IS NULL ORDER BY created_at DESC")
        .fetch_all(pool).await.map_err(ApiError::internal)?))
}

async fn account_invitation_permission(State(state): State<AppState>, current: CurrentUser) -> ApiResult<AccountInvitationPermissionResponse> {
    let can_create = can_create_account_invitations(database(&state)?, current.id).await?;
    Ok(Json(AccountInvitationPermissionResponse { can_create }))
}

async fn create_account_invitation(State(state): State<AppState>, current: CurrentUser) -> ApiResult<AccountInvitationResponse> {
    let pool = database(&state)?;
    ensure_account_invitation_creator(pool, current.id).await?;
    let token = new_token();
    let invitation: AccountInvitationResponse = sqlx::query_as("INSERT INTO account_invitations (id, token_hash, invited_by, expires_at) VALUES ($1, $2, $3, $4) RETURNING id, expires_at::text AS expires_at, NULL::text AS token")
        .bind(Uuid::new_v4()).bind(token_hash(&token)).bind(current.id).bind(Utc::now() + chrono::Duration::days(7))
        .fetch_one(pool).await.map_err(ApiError::internal)?;
    record_audit(pool, current.id, None, None, "account_invitation.created").await;
    Ok(Json(AccountInvitationResponse { token: Some(token), ..invitation }))
}

async fn revoke_account_invitation(State(state): State<AppState>, current: CurrentUser, Path(invitation_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    let result = sqlx::query("UPDATE account_invitations SET revoked_at = now() WHERE id = $1 AND accepted_at IS NULL AND revoked_at IS NULL").bind(invitation_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::bad_request("Invitation is unavailable.")); }
    record_audit(pool, current.id, None, None, "account_invitation.revoked").await;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_admin_workspaces(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<AdminWorkspaceResponse>> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    Ok(Json(sqlx::query_as("SELECT w.id, w.name, COALESCE(owner.username, 'deleted') AS owner_username, COUNT(m.user_id) AS member_count, w.archived_at FROM workspaces w LEFT JOIN users owner ON owner.id = w.created_by LEFT JOIN workspace_members m ON m.workspace_id = w.id GROUP BY w.id, owner.username ORDER BY w.created_at DESC")
        .fetch_all(pool).await.map_err(ApiError::internal)?))
}

async fn archive_workspace(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> ApiResult<AdminWorkspaceResponse> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
    let workspace: AdminWorkspaceResponse = sqlx::query_as("UPDATE workspaces w SET archived_at = CASE WHEN archived_at IS NULL THEN now() ELSE NULL END WHERE w.id = $1 RETURNING w.id, w.name, COALESCE((SELECT username FROM users WHERE id = w.created_by), 'deleted') AS owner_username, (SELECT COUNT(*) FROM workspace_members WHERE workspace_id = w.id) AS member_count, w.archived_at")
        .bind(workspace_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Workspace is unavailable."))?;
    record_audit(pool, current.id, Some(workspace_id), None, if workspace.archived_at.is_some() { "workspace.archived" } else { "workspace.restored" }).await;
    let _ = state.events.send(());
    Ok(Json(workspace))
}

async fn delete_workspace(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    let keys = sqlx::query_scalar::<_, String>("SELECT a.object_key FROM attachments a JOIN cards c ON c.id = a.card_id JOIN boards b ON b.id = c.board_id WHERE b.workspace_id = $1 AND a.object_key IS NOT NULL UNION ALL SELECT cb.object_key FROM card_backgrounds cb JOIN cards c ON c.id = cb.card_id JOIN boards b ON b.id = c.board_id WHERE b.workspace_id = $1 UNION ALL SELECT bs.object_key FROM board_stickers bs JOIN boards b ON b.id = bs.board_id WHERE b.workspace_id = $1 UNION ALL SELECT wb.object_key FROM workspace_backgrounds wb WHERE wb.workspace_id = $1")
        .bind(workspace_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let deleted = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 { return Err(ApiError::bad_request("Workspace is unavailable.")); }
    for key in keys { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    record_audit(pool, current.id, None, None, "workspace.deleted").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_workspace_background(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>, Json(request): Json<UpdateBoardBackgroundRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_workspace_background_owner(pool, workspace_id, current.id).await?;
    let url = match request.background_image_url {
        Some(value) if !value.trim().is_empty() => {
            let value = value.trim();
            if value.len() > 2_000 || !(value.starts_with("https://") || value.starts_with("/v1/workspaces/")) { return Err(ApiError::bad_request("Workspace background must be an HTTPS image URL or an uploaded Flowboard file.")); }
            Some(value.to_owned())
        }
        _ => None,
    };
    let uploaded_url = format!("/v1/workspaces/{workspace_id}/background/file");
    let previous_key = if url.as_deref() == Some(uploaded_url.as_str()) { None } else {
        sqlx::query_scalar::<_, Option<String>>("SELECT object_key FROM workspace_backgrounds WHERE workspace_id = $1")
            .bind(workspace_id).fetch_optional(pool).await.map_err(ApiError::internal)?.flatten()
    };
    let updated = sqlx::query("UPDATE workspaces SET background_image_url = $1 WHERE id = $2 AND archived_at IS NULL")
        .bind(url).bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "workspace_not_found", "Workspace was not found.".to_owned())); }
    if let Some(previous_key) = previous_key {
        sqlx::query("DELETE FROM workspace_backgrounds WHERE workspace_id = $1").bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
        let _ = tokio::fs::remove_file(state.upload_dir.join(previous_key)).await;
    }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_workspace_background_owner(pool: &PgPool, workspace_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let is_owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.role = 'owner')")
        .bind(workspace_id).bind(actor_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if is_owner { Ok(()) } else { Err(ApiError::forbidden("Only the workspace owner can change its card background.")) }
}

async fn upload_workspace_background(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<BoardBackgroundUploadResponse> {
    let pool = database(&state)?;
    ensure_workspace_background_owner(pool, workspace_id, current.id).await?;
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Workspace background upload form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Workspace background image file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Workspace background image field must be named file.")); }
    let original_name = field.file_name().unwrap_or("workspace-background").replace(['/', '\\'], "_");
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_default();
    if !matches!(media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp") {
        return Err(ApiError::bad_request("Workspace background must be a JPEG, PNG, GIF, or WebP image."));
    }
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Workspace background image could not be read."))?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 { return Err(ApiError::bad_request("Workspace background must be between 1 byte and 50 MiB.")); }
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Workspace background image type is unsupported."))?;
    let object_key = format!("workspace-background-{}.{}", Uuid::new_v4(), extension);
    let path = state.upload_dir.join(&object_key);
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "workspace background write failed"); ApiError::storage() })?;
    let previous_key = sqlx::query_scalar::<_, Option<String>>("SELECT object_key FROM workspace_backgrounds WHERE workspace_id = $1")
        .bind(workspace_id).fetch_optional(pool).await.map_err(ApiError::internal)?.flatten();
    let url = format!("/v1/workspaces/{workspace_id}/background/file");
    let result = sqlx::query("INSERT INTO workspace_backgrounds (workspace_id, uploaded_by, object_key, original_name, media_type, byte_size) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (workspace_id) DO UPDATE SET uploaded_by = EXCLUDED.uploaded_by, object_key = EXCLUDED.object_key, original_name = EXCLUDED.original_name, media_type = EXCLUDED.media_type, byte_size = EXCLUDED.byte_size, created_at = now()")
        .bind(workspace_id).bind(current.id).bind(&object_key).bind(&original_name).bind(&media_type).bind(bytes.len() as i64).execute(pool).await;
    if let Err(error) = result { let _ = tokio::fs::remove_file(&path).await; return Err(ApiError::internal(error)); }
    sqlx::query("UPDATE workspaces SET background_image_url = $1 WHERE id = $2 AND archived_at IS NULL")
        .bind(&url).bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
    if let Some(previous_key) = previous_key { let _ = tokio::fs::remove_file(state.upload_dir.join(previous_key)).await; }
    let _ = state.events.send(());
    Ok(Json(BoardBackgroundUploadResponse { url: format!("{url}?v={}", Uuid::new_v4()) }))
}

async fn download_workspace_background(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> Result<Response, ApiError> {
    let background = sqlx::query_as::<_, (String, String)>("SELECT wb.object_key, wb.media_type FROM workspace_backgrounds wb INNER JOIN workspaces w ON w.id = wb.workspace_id WHERE wb.workspace_id = $1 AND w.archived_at IS NULL AND (EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = w.id AND wm.user_id = $2) OR EXISTS (SELECT 1 FROM boards b JOIN board_members bm ON bm.board_id = b.id WHERE b.workspace_id = w.id AND b.archived_at IS NULL AND bm.user_id = $2))")
        .bind(workspace_id).bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "workspace_background_not_found", "Workspace background was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(background.0)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "workspace_background_not_found", "Workspace background file was not found.".to_owned()) }
        else { tracing::error!(?error, "workspace background read failed"); ApiError::storage() }
    })?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&background.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"))], bytes).into_response())
}

async fn list_workspaces(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<WorkspaceResponse>> {
    let actor_id = current.id;
    let rows = sqlx::query_as::<_, WorkspaceResponse>(
        "SELECT w.id, w.name, w.background_image_url, (EXISTS (SELECT 1 FROM users u WHERE u.id = $1 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members owner_member WHERE owner_member.workspace_id = w.id AND owner_member.user_id = $1 AND owner_member.role = 'owner')) AS can_manage FROM workspaces w WHERE w.archived_at IS NULL AND (EXISTS (SELECT 1 FROM users u WHERE u.id = $1 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members m WHERE m.workspace_id = w.id AND m.user_id = $1) OR EXISTS (SELECT 1 FROM boards b JOIN board_members bm ON bm.board_id = b.id WHERE b.workspace_id = w.id AND b.archived_at IS NULL AND bm.user_id = $1)) ORDER BY w.created_at DESC",
    )
    .bind(actor_id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

async fn list_workspace_members(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> ApiResult<Vec<MemberResponse>> {
    let actor_id = current.id;
    let members = sqlx::query_as::<_, MemberResponse>(
        "SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM workspace_members wm INNER JOIN users u ON u.id = wm.user_id INNER JOIN workspace_members access ON access.workspace_id = wm.workspace_id WHERE wm.workspace_id = $1 AND access.user_id = $2 ORDER BY u.display_name",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(members))
}

async fn ensure_workspace_owner(pool: &PgPool, workspace_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let is_owner: bool = sqlx::query_scalar("SELECT flowboard_has_permission($1, $2, 'manage_permissions'::workspace_permission)")
        .bind(workspace_id)
        .bind(actor_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    if is_owner { Ok(()) } else { Err(ApiError::forbidden("Permission to manage workspace access is required.")) }
}

/// Administrative project controls are intentionally narrower than a custom
/// `manage_permissions` grant: only owners, Full Access members and the
/// system owner may expose a card publicly or use the project's `…` menu.
async fn ensure_workspace_full_access(pool: &PgPool, workspace_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if allowed { Ok(()) } else { Err(ApiError::forbidden("Full Access is required for this administrative action.")) }
}

async fn ensure_board_full_access(pool: &PgPool, board_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT workspace_id FROM boards WHERE id = $1 AND archived_at IS NULL")
        .bind(board_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Project was not found."))?;
    ensure_workspace_full_access(pool, workspace_id, actor_id).await
}

async fn ensure_card_full_access(pool: &PgPool, card_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT b.workspace_id FROM cards c INNER JOIN boards b ON b.id = c.board_id WHERE c.id = $1 AND c.archived_at IS NULL AND b.archived_at IS NULL")
        .bind(card_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Card was not found."))?;
    ensure_workspace_full_access(pool, workspace_id, actor_id).await
}

async fn ensure_list_full_access(pool: &PgPool, list_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT b.workspace_id FROM lists l INNER JOIN boards b ON b.id = l.board_id WHERE l.id = $1 AND b.archived_at IS NULL")
        .bind(list_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Column was not found."))?;
    ensure_workspace_full_access(pool, workspace_id, actor_id).await
}

/// `full_access` is an administrative preset, but it is deliberately not an
/// owner-equivalent role.  Without this guard a full-access member could
/// remove or demote themself (and lock the workspace into an unexpected
/// state), or mutate another full-access member.  Only the workspace owner
/// and the system owner may manage that protected level.
async fn ensure_member_role_mutation_allowed(
    pool: &PgPool,
    workspace_id: Uuid,
    actor_id: Uuid,
    target_id: Uuid,
    requested_preset: Option<&str>,
) -> Result<(), ApiError> {
    let is_system_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_system_owner AND disabled_at IS NULL)",
    )
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if is_system_owner {
        return Ok(());
    }

    let actor_role = sqlx::query_scalar::<_, String>(
        "SELECT role::text FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    let target_role = sqlx::query_scalar::<_, String>(
        "SELECT role::text FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(target_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;

    let actor_is_workspace_owner = actor_role.as_deref() == Some("owner");
    let protects_target = target_role.as_deref() == Some("full_access")
        || requested_preset == Some("full_access");
    if !actor_is_workspace_owner && protects_target {
        return Err(ApiError::forbidden(
            "Only the workspace owner or system owner may manage Full Access members.",
        ));
    }

    Ok(())
}

async fn ensure_board_permission(pool: &PgPool, board_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))")
        .bind(board_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
}

async fn ensure_card_access_permission(pool: &PgPool, card_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards c JOIN boards b ON b.id = c.board_id LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE c.id = $1 AND c.archived_at IS NULL AND b.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))")
        .bind(card_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
}

async fn ensure_card_unfrozen(pool: &PgPool, card_id: Uuid) -> Result<(), ApiError> {
    let is_frozen = sqlx::query_scalar::<_, bool>("SELECT is_frozen FROM cards WHERE id = $1 AND archived_at IS NULL")
        .bind(card_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Card was not found."))?;
    if is_frozen {
        return Err(ApiError::forbidden("This card is frozen. A Full Access member must unfreeze it before changes can be made."));
    }
    Ok(())
}

fn card_preset_rank(preset: &str) -> i8 {
    match preset {
        "viewer" => 0,
        "contributor" => 1,
        "editor" => 2,
        "full_access" => 3,
        "owner" => 4,
        _ => -1,
    }
}

/// Workspace owners, Full Access members and the system owner retain an
/// administrative bypass. Everyone else must both belong to the board and
/// meet the card's configured workspace-role threshold.
async fn actor_meets_card_preset(pool: &PgPool, card_id: Uuid, actor_id: Uuid, required_preset: &str) -> Result<bool, ApiError> {
    let access = sqlx::query_as::<_, (Uuid, bool)>(
        "SELECT b.workspace_id, EXISTS(SELECT 1 FROM board_members bm WHERE bm.board_id = b.id AND bm.user_id = $2) FROM cards c INNER JOIN boards b ON b.id = c.board_id WHERE c.id = $1 AND b.archived_at IS NULL",
    )
    .bind(card_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    let Some((workspace_id, is_board_member)) = access else { return Ok(false); };

    let privileged: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if privileged { return Ok(true); }
    if !is_board_member { return Ok(false); }

    let preset = sqlx::query_scalar::<_, String>(
        "SELECT role::text FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(preset.is_some_and(|preset| card_preset_rank(&preset) >= card_preset_rank(required_preset)))
}

async fn ensure_card_edit_threshold(pool: &PgPool, card_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let required = sqlx::query_scalar::<_, String>("SELECT min_edit_preset FROM cards WHERE id = $1 AND archived_at IS NULL")
        .bind(card_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Card was not found."))?;
    if actor_meets_card_preset(pool, card_id, actor_id, &required).await? { Ok(()) }
    else { Err(ApiError::forbidden("Your role is below this card's editing threshold.")) }
}

/// All ordinary mutations go through this guard. A frozen card is deliberately
/// immutable even to Full Access until that user explicitly unfreezes it.
async fn ensure_card_permission(pool: &PgPool, card_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    ensure_card_access_permission(pool, card_id, actor_id, permission).await?;
    ensure_card_edit_threshold(pool, card_id, actor_id).await?;
    ensure_card_unfrozen(pool, card_id).await
}

async fn ensure_archived_card_permission(pool: &PgPool, card_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards c JOIN boards b ON b.id = c.board_id LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE c.id = $1 AND c.archived_at IS NOT NULL AND b.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))")
        .bind(card_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
}

async fn ensure_list_permission(pool: &PgPool, list_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lists l JOIN boards b ON b.id = l.board_id JOIN board_members bm ON bm.board_id = b.id WHERE l.id = $1 AND b.archived_at IS NULL AND bm.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission))")
        .bind(list_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
}

async fn list_workspace_members_for_management(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> ApiResult<Vec<WorkspaceMemberManagementResponse>> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    let members = sqlx::query_as::<_, WorkspaceMemberManagementResponse>(
        "SELECT u.id, u.username, wm.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM workspace_members wm INNER JOIN users u ON u.id = wm.user_id WHERE wm.workspace_id = $1 ORDER BY CASE wm.role WHEN 'owner' THEN 0 WHEN 'full_access' THEN 1 ELSE 2 END, u.username",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(members))
}

async fn update_workspace_member(State(state): State<AppState>, current: CurrentUser, Path((workspace_id, user_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateWorkspaceMemberRequest>) -> ApiResult<WorkspaceMemberManagementResponse> {
    let preset = match request.preset.as_str() {
        "viewer" | "contributor" | "editor" | "full_access" => request.preset,
        _ => return Err(ApiError::bad_request("preset must be viewer, contributor, editor, or full_access.")),
    };
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    ensure_member_role_mutation_allowed(pool, workspace_id, current.id, user_id, Some(&preset)).await?;
    let member = sqlx::query_as::<_, WorkspaceMemberManagementResponse>(
        "UPDATE workspace_members wm SET role = $1::workspace_role FROM users u WHERE wm.workspace_id = $2 AND wm.user_id = $3 AND wm.role <> 'owner' AND u.id = wm.user_id RETURNING u.id, u.username, wm.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url",
    )
    .bind(&preset)
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::bad_request("Workspace owner role cannot be changed."))?;
    if preset == "viewer" {
        sqlx::query("DELETE FROM workspace_member_permissions WHERE workspace_id = $1 AND user_id = $2")
            .bind(workspace_id).bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    }
    record_audit(pool, current.id, Some(workspace_id), Some(user_id), "workspace_member.preset_changed").await;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn remove_workspace_member(State(state): State<AppState>, current: CurrentUser, Path((workspace_id, user_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    ensure_member_role_mutation_allowed(pool, workspace_id, current.id, user_id, None).await?;
    let result = sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND role <> 'owner'")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::bad_request("Workspace owner cannot be removed.")); }
    record_audit(pool, current.id, Some(workspace_id), Some(user_id), "workspace_member.removed").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_available_workspace_accounts(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> ApiResult<Vec<MemberResponse>> {
    ensure_workspace_owner(database(&state)?, workspace_id, current.id).await?;
    let accounts = sqlx::query_as::<_, MemberResponse>("SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM users u WHERE u.password_hash IS NOT NULL AND u.disabled_at IS NULL AND NOT EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.user_id = u.id) ORDER BY u.username")
        .bind(workspace_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(accounts))
}

async fn add_existing_workspace_member(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>, Json(request): Json<AddExistingWorkspaceMemberRequest>) -> ApiResult<MemberResponse> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    let member = sqlx::query_as::<_, MemberResponse>("WITH account AS (SELECT id, display_name, avatar_key FROM users WHERE id = $1 AND password_hash IS NOT NULL AND disabled_at IS NULL), joined AS (INSERT INTO workspace_members (workspace_id, user_id, role) SELECT $2, id, 'viewer' FROM account ON CONFLICT (workspace_id, user_id) DO NOTHING RETURNING user_id) SELECT id, display_name, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM account")
        .bind(request.user_id).bind(workspace_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Account is unavailable."))?;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn list_board_members_for_management(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<WorkspaceMemberManagementResponse>> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
    Ok(Json(sqlx::query_as("SELECT u.id, u.username, wm.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM board_members bm JOIN boards b ON b.id = bm.board_id JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id JOIN users u ON u.id = bm.user_id WHERE bm.board_id = $1 ORDER BY CASE wm.role WHEN 'owner' THEN 0 WHEN 'full_access' THEN 1 ELSE 2 END, u.username")
        .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?))
}

async fn list_available_board_accounts(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<MemberResponse>> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "invite_members").await?;
    Ok(Json(sqlx::query_as("SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM users u WHERE u.password_hash IS NOT NULL AND u.disabled_at IS NULL AND NOT EXISTS (SELECT 1 FROM board_members bm WHERE bm.board_id = $1 AND bm.user_id = u.id) ORDER BY u.username")
        .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?))
}

async fn add_existing_board_member(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<AddExistingWorkspaceMemberRequest>) -> ApiResult<MemberResponse> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "invite_members").await?;
    let member = sqlx::query_as::<_, MemberResponse>("WITH account AS (SELECT id, display_name, avatar_key FROM users WHERE id = $1 AND password_hash IS NOT NULL AND disabled_at IS NULL), workspace AS (SELECT workspace_id FROM boards WHERE id = $2 AND archived_at IS NULL), workspace_member AS (INSERT INTO workspace_members (workspace_id, user_id, role) SELECT workspace.workspace_id, account.id, 'viewer' FROM workspace, account ON CONFLICT (workspace_id, user_id) DO NOTHING), joined AS (INSERT INTO board_members (board_id, user_id, role) SELECT $2, account.id, 'viewer' FROM account ON CONFLICT (board_id, user_id) DO NOTHING) SELECT id, display_name, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM account")
        .bind(request.user_id).bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Account is unavailable."))?;
    record_audit(pool, current.id, None, Some(request.user_id), "project_member.added").await;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn update_board_member(State(state): State<AppState>, current: CurrentUser, Path((board_id, user_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateWorkspaceMemberRequest>) -> ApiResult<WorkspaceMemberManagementResponse> {
    let preset = match request.preset.as_str() {
        "viewer" | "contributor" | "editor" | "full_access" => request.preset,
        _ => return Err(ApiError::bad_request("preset must be viewer, contributor, editor, or full_access.")),
    };
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
    let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT workspace_id FROM boards WHERE id = $1")
        .bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Project was not found."))?;
    ensure_member_role_mutation_allowed(pool, workspace_id, current.id, user_id, Some(&preset)).await?;
    let member = sqlx::query_as::<_, WorkspaceMemberManagementResponse>("WITH target AS (SELECT b.workspace_id FROM boards b WHERE b.id = $2), updated AS (UPDATE workspace_members wm SET role = $1::workspace_role FROM target WHERE wm.workspace_id = target.workspace_id AND wm.user_id = $3 AND wm.role <> 'owner' AND EXISTS (SELECT 1 FROM board_members bm WHERE bm.board_id = $2 AND bm.user_id = wm.user_id) RETURNING wm.user_id, wm.role) UPDATE board_members bm SET role = CASE WHEN $1 = 'viewer' THEN 'viewer'::board_role ELSE 'editor'::board_role END FROM updated, users u WHERE bm.board_id = $2 AND bm.user_id = updated.user_id AND u.id = updated.user_id RETURNING u.id, u.username, updated.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url")
        .bind(&preset).bind(board_id).bind(user_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Project owner role cannot be changed."))?;
    if preset == "viewer" {
        let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT workspace_id FROM boards WHERE id = $1")
            .bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM workspace_member_permissions WHERE workspace_id = $1 AND user_id = $2")
            .bind(workspace_id).bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    }
    record_audit(pool, current.id, None, Some(user_id), "project_member.preset_changed").await;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn remove_board_member(State(state): State<AppState>, current: CurrentUser, Path((board_id, user_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "remove_members").await?;
    let workspace_id = sqlx::query_scalar::<_, Uuid>("SELECT workspace_id FROM boards WHERE id = $1")
        .bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Project was not found."))?;
    ensure_member_role_mutation_allowed(pool, workspace_id, current.id, user_id, None).await?;
    let result = sqlx::query("DELETE FROM board_members bm USING boards b, workspace_members wm WHERE bm.board_id = $1 AND bm.user_id = $2 AND b.id = bm.board_id AND wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id AND wm.role <> 'owner'")
        .bind(board_id).bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::bad_request("Project owner cannot be removed.")); }
    sqlx::query("DELETE FROM workspace_members wm USING boards b WHERE b.id = $1 AND wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role <> 'owner' AND NOT EXISTS (SELECT 1 FROM board_members bm JOIN boards member_board ON member_board.id = bm.board_id WHERE member_board.workspace_id = wm.workspace_id AND bm.user_id = wm.user_id)")
        .bind(board_id).bind(user_id).execute(pool).await.map_err(ApiError::internal)?;
    record_audit(pool, current.id, None, Some(user_id), "project_member.removed").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

const WORKSPACE_PERMISSIONS: [&str; 10] = ["create_cards", "edit_cards", "delete_cards", "create_lists", "delete_lists", "create_labels", "delete_labels", "invite_members", "remove_members", "manage_permissions"];

async fn list_member_permissions(State(state): State<AppState>, current: CurrentUser, Path((workspace_id, user_id)): Path<(Uuid, Uuid)>) -> ApiResult<MemberPermissionsResponse> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    let permissions = sqlx::query_scalar::<_, String>("SELECT permission::text FROM workspace_member_permissions WHERE workspace_id = $1 AND user_id = $2 ORDER BY permission::text")
        .bind(workspace_id).bind(user_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(MemberPermissionsResponse { user_id, permissions }))
}

async fn replace_member_permissions(State(state): State<AppState>, current: CurrentUser, Path((workspace_id, user_id)): Path<(Uuid, Uuid)>, Json(request): Json<ReplaceMemberPermissionsRequest>) -> ApiResult<MemberPermissionsResponse> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
    ensure_member_role_mutation_allowed(pool, workspace_id, current.id, user_id, None).await?;
    let permissions: Vec<String> = request.permissions.into_iter().collect::<HashSet<_>>().into_iter().collect();
    if permissions.len() > WORKSPACE_PERMISSIONS.len() || permissions.iter().any(|permission| !WORKSPACE_PERMISSIONS.contains(&permission.as_str())) {
        return Err(ApiError::bad_request("Unknown workspace permission."));
    }
    let member_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND role <> 'owner')")
        .bind(workspace_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !member_exists { return Err(ApiError::bad_request("Permissions for workspace owner cannot be edited.")); }
    let is_viewer: bool = sqlx::query_scalar("SELECT role = 'viewer' FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if is_viewer && !permissions.is_empty() {
        return Err(ApiError::bad_request("Viewer is read-only; choose another role before granting permissions."));
    }
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM workspace_member_permissions WHERE workspace_id = $1 AND user_id = $2").bind(workspace_id).bind(user_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for permission in &permissions {
        sqlx::query("INSERT INTO workspace_member_permissions (workspace_id, user_id, permission, granted_by) VALUES ($1, $2, $3::workspace_permission, $4)")
            .bind(workspace_id).bind(user_id).bind(permission).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    record_audit(pool, current.id, Some(workspace_id), Some(user_id), "workspace_member.permissions_changed").await;
    let _ = state.events.send(());
    Ok(Json(MemberPermissionsResponse { user_id, permissions }))
}

async fn create_workspace(State(state): State<AppState>, current: CurrentUser, Json(request): Json<CreateWorkspaceRequest>) -> ApiResult<WorkspaceResponse> {
    let actor_id = current.id;
    let name = valid_text(&request.name, "name", 120)?;
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let workspace = sqlx::query_as::<_, WorkspaceResponse>(
        "INSERT INTO workspaces (id, name, created_by) VALUES ($1, $2, $3) RETURNING id, name, background_image_url, TRUE AS can_manage",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(actor_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) SELECT $1, u.id, 'owner' FROM users u WHERE u.id = $2 OR (u.is_system_owner AND u.disabled_at IS NULL) ON CONFLICT (workspace_id, user_id) DO NOTHING")
        .bind(workspace.id)
        .bind(actor_id)
        .execute(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_audit(pool, actor_id, Some(workspace.id), None, "workspace.created").await;
    Ok(Json(workspace))
}

async fn list_boards(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>) -> ApiResult<Vec<BoardSummary>> {
    let actor_id = current.id;
    let rows = sqlx::query_as::<_, BoardSummary>(
        "SELECT b.id, b.title, b.visibility::text AS visibility FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE b.workspace_id = $1 AND m.user_id = $2 AND b.archived_at IS NULL ORDER BY b.created_at DESC",
    )
    .bind(workspace_id)
    .bind(actor_id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(rows))
}

async fn create_board(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>, Json(request): Json<CreateBoardRequest>) -> ApiResult<BoardSummary> {
    ensure_workspace_owner(database(&state)?, workspace_id, current.id).await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 200)?;
    let pool = database(&state)?;
    let membership_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2)")
        .bind(workspace_id)
        .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if !membership_exists { return Err(ApiError(StatusCode::NOT_FOUND, "workspace_not_found", "Workspace was not found.".to_owned())); }
    let board = sqlx::query_as::<_, BoardSummary>(
        "INSERT INTO boards (id, workspace_id, title, created_by) VALUES ($1, $2, $3, $4) RETURNING id, title, visibility::text AS visibility",
    )
    .bind(Uuid::new_v4())
    .bind(workspace_id)
    .bind(title)
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO board_members (board_id, user_id, role) SELECT $1, wm.user_id, 'editor' FROM workspace_members wm WHERE wm.workspace_id = $2 AND wm.role = 'owner' ON CONFLICT (board_id, user_id) DO NOTHING")
        .bind(board.id).bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(board))
}

async fn get_board(State(state): State<AppState>, current: Viewer, Path(board_id): Path<Uuid>) -> ApiResult<BoardDetail> {
    let actor_id = current.0.map(|user| user.id);
    let pool = database(&state)?;
    let board = sqlx::query_as::<_, BoardAccess>(
        "SELECT b.id, b.workspace_id, b.title, CASE WHEN b.background_image_url = '/v1/boards/' || b.id::text || '/background/file' THEN b.background_image_url || '?v=' || (floor(EXTRACT(EPOCH FROM b.updated_at) * 1000)::bigint)::text ELSE b.background_image_url END AS background_image_url, b.background_fit, b.background_position, b.visibility::text AS visibility FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public')",
    )
    .bind(board_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let lists = sqlx::query_as::<_, ListResponse>("SELECT id, title, grid_column, grid_row, card_limit, is_public FROM lists WHERE board_id = $1 AND ($2::uuid IS NOT NULL OR is_public) ORDER BY position, id")
        .bind(board_id)
        .bind(actor_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let cards = sqlx::query_as::<_, BoardCardRow>("SELECT c.id, c.list_id, c.title, c.description, c.priority, c.is_frozen, (SELECT MAX(ca.created_at)::text FROM card_activity ca WHERE ca.card_id = c.id) AS last_activity_at, c.is_public, c.min_view_preset, c.min_edit_preset, ($2::uuid IS NOT NULL AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) AND (EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access')) OR EXISTS(SELECT 1 FROM board_members bm INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = b.id AND bm.user_id = $2 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END >= CASE c.min_edit_preset WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 1 END))) AS can_edit, c.background_image_url, c.start_at::text AS start_at, c.due_at::text AS due_at, c.cover_attachment_id, CASE WHEN a.id IS NULL THEN NULL ELSE '/v1/attachments/' || a.id::text || '/content' END AS cover_url, a.media_type AS cover_media_type, c.cover_mode, c.completed_at::text AS completed_at, (SELECT COUNT(*) FROM checklist_items ci WHERE ci.card_id = c.id) AS checklist_total, (SELECT COUNT(*) FROM checklist_items ci WHERE ci.card_id = c.id AND ci.is_completed) AS checklist_completed, (SELECT COUNT(*) FROM comments cm WHERE cm.card_id = c.id) AS comment_count, (SELECT COUNT(*) FROM attachments at WHERE at.card_id = c.id AND at.checklist_item_id IS NULL) AS attachment_count, EXISTS(SELECT 1 FROM card_mentions cmn WHERE cmn.card_id = c.id AND cmn.user_id = $2 AND cmn.read_at IS NULL) AS has_unread_mentions, EXISTS(SELECT 1 FROM comments cm LEFT JOIN comment_read_states read_state ON read_state.comment_id = cm.id AND read_state.user_id = $2 LEFT JOIN users viewer ON viewer.id = $2 WHERE cm.card_id = c.id AND cm.author_id IS DISTINCT FROM $2 AND cm.created_at > COALESCE(read_state.read_at, viewer.created_at)) AS has_unread_comments, ($2::uuid IS NOT NULL AND EXISTS(SELECT 1 FROM card_polls p WHERE p.card_id = c.id AND NOT EXISTS(SELECT 1 FROM card_poll_votes v WHERE v.poll_id = p.id AND v.user_id = $2))) AS has_unvoted_polls, ms.id AS milestone_id, ms.name AS milestone_name, ms.description AS milestone_description, ms.color AS milestone_color, ms.target_date::text AS milestone_target_date FROM cards c INNER JOIN lists l ON l.id = c.list_id INNER JOIN boards b ON b.id = c.board_id LEFT JOIN attachments a ON a.id = c.cover_attachment_id LEFT JOIN milestones ms ON ms.id = c.milestone_id WHERE c.board_id = $1 AND c.archived_at IS NULL AND ($2::uuid IS NOT NULL OR l.is_public) AND ((b.visibility = 'public' AND c.is_public) OR ($2::uuid IS NOT NULL AND (EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access')) OR EXISTS(SELECT 1 FROM board_members bm INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = b.id AND bm.user_id = $2 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END >= CASE c.min_view_preset WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 0 END)))) ORDER BY c.position")
    .bind(board_id)
    .bind(actor_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let card_ids: Vec<Uuid> = cards.iter().map(|card| card.id).collect();
    let card_labels = sqlx::query_as::<_, CardLabelRow>("SELECT cl.card_id, l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN cards c ON c.id = cl.card_id INNER JOIN labels l ON l.id = cl.label_id AND l.board_id = c.board_id WHERE cl.card_id = ANY($1) ORDER BY l.name")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let mut card_assignees = sqlx::query_as::<_, CardAssigneeRow>("SELECT ca.card_id, u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM card_assignees ca INNER JOIN users u ON u.id = ca.user_id WHERE ca.card_id = ANY($1) ORDER BY u.display_name")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let card_roles = sqlx::query_as::<_, CardProfileRoleRow>("SELECT cpr.card_id, pr.id, pr.name, pr.color, pr.icon_shape, pr.icon_color FROM card_profile_roles cpr INNER JOIN profile_roles pr ON pr.id = cpr.role_id WHERE cpr.card_id = ANY($1) ORDER BY pr.name")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let card_waiting = sqlx::query_as::<_, CardWaitingResponse>("SELECT cw.card_id, cw.user_id, u.display_name AS user_name, cw.role_id, pr.name AS role_name, pr.color AS role_color, cw.note FROM card_waiting_for cw LEFT JOIN users u ON u.id = cw.user_id LEFT JOIN profile_roles pr ON pr.id = cw.role_id WHERE cw.card_id = ANY($1)")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    if actor_id.is_none() {
        for assignee in &mut card_assignees {
            if assignee.avatar_url.is_some() { assignee.avatar_url = Some(format!("/v1/public/boards/{}/avatars/{}", board.id, assignee.id)); }
        }
    }
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT id, name, color, icon_shape, icon_color FROM labels WHERE board_id = $1 ORDER BY name")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let milestones = sqlx::query_as::<_, MilestoneResponse>("SELECT id, name, description, color, target_date::text AS target_date FROM milestones WHERE board_id = $1 ORDER BY target_date NULLS LAST, name")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let stickers = sqlx::query_as::<_, BoardStickerRow>("SELECT id, name, media_type FROM board_stickers WHERE board_id = $1 ORDER BY created_at, id")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|sticker| board_sticker_response(board.id, sticker))
        .collect();
    let cards: Vec<BoardCard> = cards.into_iter().map(|card| BoardCard {
        id: card.id,
        list_id: card.list_id,
        title: card.title,
        description: card.description,
        priority: card.priority,
        is_frozen: card.is_frozen,
        last_activity_at: card.last_activity_at,
        is_public: card.is_public,
        min_view_preset: card.min_view_preset,
        min_edit_preset: card.min_edit_preset,
        can_edit: card.can_edit,
        background_image_url: card.background_image_url,
        start_at: card.start_at,
        due_at: card.due_at,
        cover_attachment_id: card.cover_attachment_id,
        cover_url: card.cover_url,
        cover_media_type: card.cover_media_type,
        cover_mode: card.cover_mode,
        completed_at: card.completed_at,
        checklist_total: card.checklist_total,
        checklist_completed: card.checklist_completed,
        comment_count: card.comment_count,
        attachment_count: card.attachment_count,
        has_unread_mentions: card.has_unread_mentions,
        has_unread_comments: card.has_unread_comments,
        has_unvoted_polls: card.has_unvoted_polls,
        milestone: card.milestone_id.map(|id| MilestoneResponse { id, name: card.milestone_name.unwrap_or_default(), description: card.milestone_description.unwrap_or_default(), color: card.milestone_color.unwrap_or_else(|| "#6ea8fe".to_owned()), target_date: card.milestone_target_date }),
        waiting: card_waiting.iter().find(|waiting| waiting.card_id == card.id).cloned(),
        labels: card_labels.iter().filter(|label| label.card_id == card.id).map(|label| LabelResponse { id: label.id, name: label.name.clone(), color: label.color.clone(), icon_shape: label.icon_shape.clone(), icon_color: label.icon_color.clone() }).collect(),
        roles: card_roles.iter().filter(|role| role.card_id == card.id).map(|role| ProfileRoleResponse { id: role.id, name: role.name.clone(), color: role.color.clone(), icon_shape: role.icon_shape.clone(), icon_color: role.icon_color.clone() }).collect(),
        assignees: card_assignees.iter().filter(|member| member.card_id == card.id).map(|member| MemberResponse { id: member.id, display_name: member.display_name.clone(), avatar_url: member.avatar_url.clone() }).collect(),
    }).collect();
    let mut members = sqlx::query_as::<_, MemberResponse>("SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM board_members bm INNER JOIN users u ON u.id = bm.user_id WHERE bm.board_id = $1 AND u.password_hash IS NOT NULL AND u.disabled_at IS NULL ORDER BY u.display_name")
        .bind(board.id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    if actor_id.is_none() {
        for member in &mut members {
            if member.avatar_url.is_some() { member.avatar_url = Some(format!("/v1/public/boards/{}/avatars/{}", board.id, member.id)); }
        }
    }
    let lists = lists.into_iter().map(|list| BoardList {
        id: list.id,
        title: list.title,
        grid_column: list.grid_column,
        grid_row: list.grid_row,
        card_limit: list.card_limit,
        is_public: list.is_public,
        cards: cards.iter().filter(|card| card.list_id == list.id).cloned().collect(),
    }).collect();
    let uploaded_background_url = format!("/v1/boards/{}/background/file", board.id);
    let background_image_url = if actor_id.is_none() && board.background_image_url.as_deref().is_some_and(|url| url.starts_with(&uploaded_background_url)) {
        Some(format!("/v1/public/boards/{}/background", board.id))
    } else { board.background_image_url };
    let can_edit = match actor_id {
        Some(user_id) => sqlx::query_scalar::<_, bool>("SELECT flowboard_has_permission($2, $3, 'edit_cards'::workspace_permission) AND (EXISTS(SELECT 1 FROM board_members WHERE board_id = $1 AND user_id = $3) OR EXISTS(SELECT 1 FROM users WHERE id = $3 AND is_system_owner AND disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id = $2 AND user_id = $3 AND role IN ('owner', 'full_access')))")
            .bind(board.id).bind(board.workspace_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?,
        None => false,
    };
    let can_admin = match actor_id {
        Some(user_id) => sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))")
            .bind(board.workspace_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?,
        None => false,
    };
    Ok(Json(BoardDetail { id: board.id, workspace_id: board.workspace_id, title: board.title, background_image_url, background_fit: board.background_fit, background_position: board.background_position, visibility: board.visibility, can_edit, can_admin, labels, milestones, stickers, members, lists }))
}

async fn ensure_board_layout_access(pool: &PgPool, board_id: Uuid, actor_id: Uuid) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (bm.user_id IS NOT NULL OR EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))",
    )
    .bind(board_id).bind(actor_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if allowed { Ok(()) } else { Err(ApiError::forbidden("You do not have access to store a layout for this board.")) }
}

async fn get_board_layout(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<BoardLayoutResponse> {
    let pool = database(&state)?;
    ensure_board_layout_access(pool, board_id, current.id).await?;
    let view_mode = sqlx::query_scalar::<_, String>("SELECT view_mode FROM board_layout_preferences WHERE user_id = $1 AND board_id = $2")
        .bind(current.id).bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .unwrap_or_else(|| "standard".to_owned());
    let positions = sqlx::query_as::<_, BoardFreeformPositionResponse>(
        "SELECT list_id, x, y FROM board_freeform_list_positions WHERE board_id = $1 ORDER BY y, x, list_id",
    )
    .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(BoardLayoutResponse { view_mode, positions }))
}

async fn update_board_layout(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardLayoutRequest>) -> ApiResult<BoardLayoutResponse> {
    if !matches!(request.view_mode.as_str(), "standard" | "freeform" | "dependencies") {
        return Err(ApiError::bad_request("view_mode must be standard, freeform, or dependencies."));
    }
    if request.positions.len() > 500 {
        return Err(ApiError::bad_request("A board layout cannot contain more than 500 columns."));
    }
    let mut list_ids = HashSet::new();
    for position in &request.positions {
        if position.x < 0 || position.y < 0 || position.x > 200_000 || position.y > 200_000 || !list_ids.insert(position.list_id) {
            return Err(ApiError::bad_request("Each freeform position must use a unique list id and coordinates from 0 to 200000."));
        }
    }
    let pool = database(&state)?;
    ensure_board_layout_access(pool, board_id, current.id).await?;
    ensure_board_permission(pool, board_id, current.id, "create_lists").await?;
    let list_ids: Vec<Uuid> = list_ids.into_iter().collect();
    if !list_ids.is_empty() {
        let valid_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lists WHERE board_id = $1 AND id = ANY($2)")
            .bind(board_id).bind(&list_ids).fetch_one(pool).await.map_err(ApiError::internal)?;
        if valid_count != list_ids.len() as i64 {
            return Err(ApiError::bad_request("The layout contains a list from another board or a deleted list."));
        }
    }
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO board_layout_preferences (user_id, board_id, view_mode) VALUES ($1, $2, $3) ON CONFLICT (user_id, board_id) DO UPDATE SET view_mode = EXCLUDED.view_mode, updated_at = now()")
        .bind(current.id).bind(board_id).bind(&request.view_mode).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM board_freeform_list_positions WHERE board_id = $1")
        .bind(board_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for position in &request.positions {
        sqlx::query("INSERT INTO board_freeform_list_positions (board_id, list_id, x, y) VALUES ($1, $2, $3, $4)")
            .bind(board_id).bind(position.list_id).bind(position.x).bind(position.y).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(BoardLayoutResponse {
        view_mode: request.view_mode,
        positions: request.positions.into_iter().map(|position| BoardFreeformPositionResponse { list_id: position.list_id, x: position.x, y: position.y }).collect(),
    }))
}

async fn get_board_freeform_card_positions(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<BoardFreeformCardPositionResponse>> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    let positions = sqlx::query_as::<_, BoardFreeformCardPositionResponse>(
        "SELECT card_id, x, y FROM board_freeform_card_positions WHERE board_id = $1 ORDER BY updated_at DESC",
    )
    .bind(board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(positions))
}

async fn update_board_freeform_card_position(State(state): State<AppState>, current: CurrentUser, Path((board_id, card_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateBoardFreeformCardPositionRequest>) -> ApiResult<BoardFreeformCardPositionResponse> {
    if !(0..=200_000).contains(&request.x) || !(0..=200_000).contains(&request.y) {
        return Err(ApiError::bad_request("Freeform coordinates must be between 0 and 200000."));
    }
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "edit_cards").await?;
    let saved = sqlx::query_as::<_, BoardFreeformCardPositionResponse>(
        "INSERT INTO board_freeform_card_positions (board_id, card_id, x, y) SELECT $1, c.id, $2, $3 FROM cards c WHERE c.id = $4 AND c.board_id = $1 AND c.archived_at IS NULL ON CONFLICT (board_id, card_id) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y, updated_at = now() RETURNING card_id, x, y",
    )
    .bind(board_id).bind(request.x).bind(request.y).bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found in this board.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(saved))
}

async fn clear_board_freeform_card_position(State(state): State<AppState>, current: CurrentUser, Path((board_id, card_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "edit_cards").await?;
    let removed = sqlx::query("DELETE FROM board_freeform_card_positions p USING cards c WHERE p.board_id = $1 AND p.card_id = $2 AND c.id = p.card_id AND c.board_id = $1")
        .bind(board_id).bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if removed.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "freeform_card_not_found", "Card is not detached on this board.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn get_dependency_graph_node_positions(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<DependencyGraphNodePositionResponse>> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    let positions = sqlx::query_as::<_, DependencyGraphNodePositionResponse>(
        "SELECT card_id, x, y FROM board_dependency_node_positions WHERE board_id = $1 ORDER BY updated_at DESC",
    )
    .bind(board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(positions))
}

async fn update_dependency_graph_node_position(State(state): State<AppState>, current: CurrentUser, Path((board_id, card_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateDependencyGraphNodePositionRequest>) -> ApiResult<DependencyGraphNodePositionResponse> {
    if !(0..=200_000).contains(&request.x) || !(0..=200_000).contains(&request.y) {
        return Err(ApiError::bad_request("Dependency graph coordinates must be between 0 and 200000."));
    }
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "edit_cards").await?;
    let position = sqlx::query_as::<_, DependencyGraphNodePositionResponse>(
        "INSERT INTO board_dependency_node_positions (board_id, card_id, x, y) SELECT $1, c.id, $2, $3 FROM cards c WHERE c.id = $4 AND c.board_id = $1 AND c.archived_at IS NULL ON CONFLICT (board_id, card_id) DO UPDATE SET x = EXCLUDED.x, y = EXCLUDED.y, updated_at = now() RETURNING card_id, x, y",
    )
    .bind(board_id).bind(request.x).bind(request.y).bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found in this board.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(position))
}

async fn freeform_live_snapshot(state: &AppState, board_id: Uuid) -> ApiResult<FreeformLiveResponse> {
    let now = Instant::now();
    let (cursors, pings) = {
        let mut boards = state.freeform_live.lock().await;
        let board = boards.entry(board_id).or_default();
        board.cursors.retain(|_, cursor| now.duration_since(cursor.last_seen) <= Duration::from_secs(12));
        board.pings.retain(|ping| ping.expires_at > now);
        (
            board.cursors.iter().map(|(user_id, cursor)| (*user_id, cursor.x, cursor.y)).collect::<Vec<_>>(),
            board.pings.iter().map(|ping| (ping.id, ping.user_id, ping.x, ping.y, ping.expires_at.saturating_duration_since(now).as_millis().min(u64::MAX as u128) as u64)).collect::<Vec<_>>(),
        )
    };
    let user_ids = cursors.iter().map(|(user_id, _, _)| *user_id).chain(pings.iter().map(|(_, user_id, _, _, _)| *user_id)).collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
    if user_ids.is_empty() { return Ok(Json(FreeformLiveResponse { cursors: Vec::new(), pings: Vec::new() })); }
    let accounts = sqlx::query_as::<_, FreeformLiveAccount>("SELECT id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM users WHERE id = ANY($1) AND disabled_at IS NULL")
        .bind(&user_ids).fetch_all(database(state)?).await.map_err(ApiError::internal)?;
    let accounts = accounts.into_iter().map(|account| (account.id, account)).collect::<HashMap<_, _>>();
    Ok(Json(FreeformLiveResponse {
        cursors: cursors.into_iter().filter_map(|(user_id, x, y)| accounts.get(&user_id).map(|account| FreeformLiveCursorResponse { user_id, username: account.username.clone(), avatar_url: account.avatar_url.clone(), x, y })).collect(),
        pings: pings.into_iter().filter_map(|(id, user_id, x, y, expires_in_ms)| accounts.get(&user_id).map(|account| FreeformPingResponse { id, user_id, username: account.username.clone(), avatar_url: account.avatar_url.clone(), x, y, expires_in_ms })).collect(),
    }))
}

async fn get_freeform_live(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<FreeformLiveResponse> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    freeform_live_snapshot(&state, board_id).await
}

async fn board_presence_snapshot(state: &AppState, board_id: Uuid) -> ApiResult<Vec<BoardPresenceEntry>> {
    let active = {
        let now = Instant::now();
        let mut boards = state.board_presence.lock().await;
        let board = boards.entry(board_id).or_default();
        board.retain(|_, presence| now.duration_since(presence.last_seen) <= Duration::from_secs(18));
        board.iter().map(|(user_id, presence)| (*user_id, presence.clone())).collect::<Vec<_>>()
    };
    if active.is_empty() { return Ok(Json(Vec::new())); }
    let ids = active.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let accounts = sqlx::query_as::<_, FreeformLiveAccount>("SELECT id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM users WHERE id = ANY($1) AND disabled_at IS NULL")
        .bind(&ids).fetch_all(database(state)?).await.map_err(ApiError::internal)?;
    let accounts = accounts.into_iter().map(|account| (account.id, account)).collect::<HashMap<_, _>>();
    let card_ids = active.iter().filter_map(|(_, presence)| presence.card_id).collect::<Vec<_>>();
    let card_titles = if card_ids.is_empty() { HashMap::new() } else {
        sqlx::query_as::<_, (Uuid, String)>("SELECT id, title FROM cards WHERE id = ANY($1)")
            .bind(&card_ids).fetch_all(database(state)?).await.map_err(ApiError::internal)?
            .into_iter().collect::<HashMap<_, _>>()
    };
    Ok(Json(active.into_iter().filter_map(|(user_id, presence)| accounts.get(&user_id).map(|account| BoardPresenceEntry {
        user_id,
        username: account.username.clone(),
        avatar_url: account.avatar_url.clone(),
        card_id: presence.card_id,
        card_title: presence.card_id.and_then(|card_id| card_titles.get(&card_id).cloned()),
        editing_description: presence.editing_description,
        location: presence.location,
    })).collect()))
}

async fn get_board_presence(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<BoardPresenceEntry>> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    board_presence_snapshot(&state, board_id).await
}

async fn update_board_presence(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardPresenceRequest>) -> ApiResult<Vec<BoardPresenceEntry>> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    if let Some(card_id) = request.card_id {
        let belongs_to_board = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL)")
            .bind(card_id).bind(board_id).fetch_one(database(&state)?).await.map_err(ApiError::internal)?;
        if !belongs_to_board { return Err(ApiError::bad_request("The active card does not belong to this board.")); }
    }
    state.board_presence.lock().await.entry(board_id).or_default().insert(current.id, BoardPresence { card_id: request.card_id, editing_description: request.editing_description, location: request.location, last_seen: Instant::now() });
    board_presence_snapshot(&state, board_id).await
}

async fn leave_board_presence(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    let mut boards = state.board_presence.lock().await;
    if let Some(board) = boards.get_mut(&board_id) {
        board.remove(&current.id);
        if board.is_empty() { boards.remove(&board_id); }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn record_freeform_live_update(state: &AppState, board_id: Uuid, current: CurrentUser, x: i32, y: i32, ping: bool) -> Result<FreeformLiveEvent, ApiError> {
    if !(0..=200_000).contains(&x) || !(0..=200_000).contains(&y) {
        return Err(ApiError::bad_request("Freeform coordinates must be between 0 and 200000."));
    }
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    let account = sqlx::query_as::<_, FreeformLiveAccount>("SELECT id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM users WHERE id = $1 AND disabled_at IS NULL")
        .bind(current.id).fetch_optional(database(state)?).await.map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)?;
    let now = Instant::now();
    let ping_id = if ping { Some(Uuid::new_v4()) } else { None };
    {
        let mut boards = state.freeform_live.lock().await;
        let board = boards.entry(board_id).or_default();
        board.cursors.insert(current.id, FreeformCursorPresence { x, y, last_seen: now });
        if let Some(id) = ping_id {
            board.pings.push(FreeformPingPresence { id, user_id: current.id, x, y, expires_at: now + Duration::from_secs(5) });
        }
    }
    Ok(match ping_id {
        Some(id) => FreeformLiveEvent::Ping { board_id, id, user_id: current.id, username: account.username, avatar_url: account.avatar_url, x, y, expires_in_ms: 5_000 },
        None => FreeformLiveEvent::Cursor { board_id, user_id: current.id, username: account.username, avatar_url: account.avatar_url, x, y },
    })
}

async fn update_freeform_live(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateFreeformLiveRequest>) -> ApiResult<FreeformLiveResponse> {
    let event = record_freeform_live_update(&state, board_id, current, request.x, request.y, request.ping).await?;
    let _ = state.freeform_live_events.send(event);
    freeform_live_snapshot(&state, board_id).await
}

async fn freeform_live_websocket(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, upgrade: WebSocketUpgrade) -> Result<Response, ApiError> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    Ok(upgrade.on_upgrade(move |socket| run_freeform_live_websocket(state, board_id, current, socket)))
}

async fn run_freeform_live_websocket(state: AppState, board_id: Uuid, current: CurrentUser, socket: WebSocket) {
    let (mut sender, mut receiver) = futures_util::StreamExt::split(socket);
    let mut events = state.freeform_live_events.subscribe();
    loop {
        tokio::select! {
            incoming = futures_util::StreamExt::next(&mut receiver) => {
                let Some(Ok(Message::Text(payload))) = incoming else { break; };
                let Ok(request) = serde_json::from_str::<FreeformLiveSocketRequest>(&payload) else { continue; };
                let (x, y, ping) = match request {
                    FreeformLiveSocketRequest::Cursor { x, y } => (x, y, false),
                    FreeformLiveSocketRequest::Ping { x, y } => (x, y, true),
                };
                if let Ok(event) = record_freeform_live_update(&state, board_id, current, x, y, ping).await {
                    let _ = state.freeform_live_events.send(event);
                } else { break; }
            }
            event = events.recv() => {
                let Ok(event) = event else { continue; };
                let event_board_id = match &event { FreeformLiveEvent::Cursor { board_id, .. } | FreeformLiveEvent::Ping { board_id, .. } => *board_id };
                if event_board_id != board_id { continue; }
                let Ok(payload) = serde_json::to_string(&event) else { continue; };
                if sender.send(Message::Text(payload.into())).await.is_err() { break; }
            }
        }
    }
}

async fn get_board_freeform_drawing(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<BoardFreeformDrawingResponse> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    let document = sqlx::query_scalar::<_, Value>("SELECT document FROM board_freeform_drawings WHERE board_id = $1")
        .bind(board_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .unwrap_or_else(|| json!({ "strokes": [] }));
    Ok(Json(BoardFreeformDrawingResponse { document }))
}

async fn replace_board_freeform_drawing(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<ReplaceBoardFreeformDrawingRequest>) -> ApiResult<BoardFreeformDrawingResponse> {
    ensure_board_layout_access(database(&state)?, board_id, current.id).await?;
    ensure_board_permission(database(&state)?, board_id, current.id, "edit_cards").await?;
    let strokes = request.document.get("strokes").and_then(Value::as_array).ok_or_else(|| ApiError::bad_request("Drawing document must contain a strokes array."))?;
    if strokes.len() > 600 { return Err(ApiError::bad_request("A freeform drawing supports up to 600 strokes.")); }
    let point_count = strokes.iter().map(|stroke| stroke.get("points").and_then(Value::as_array).map_or(0, Vec::len)).sum::<usize>();
    if point_count > 30_000 || serde_json::to_vec(&request.document).map_or(true, |document| document.len() > 1_500_000) {
        return Err(ApiError::bad_request("The freeform drawing is too large."));
    }
    for stroke in strokes {
        match (stroke.get("id").and_then(Value::as_str), stroke.get("author_id").and_then(Value::as_str)) {
            (None, None) => {} // Legacy pre-collaboration stroke: preserve it until touched by the eraser.
            (Some(id), Some(author_id)) => {
                Uuid::parse_str(id).map_err(|_| ApiError::bad_request("Each stroke id must be a UUID."))?;
                Uuid::parse_str(author_id).map_err(|_| ApiError::bad_request("Each stroke author must be a UUID."))?;
            }
            _ => return Err(ApiError::bad_request("Each identified stroke must have both an id and an author.")),
        }
    }
    let previous = sqlx::query_scalar::<_, Value>("SELECT document FROM board_freeform_drawings WHERE board_id = $1")
        .bind(board_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .unwrap_or_else(|| json!({ "strokes": [] }));
    let previous_strokes = previous.get("strokes").and_then(Value::as_array).cloned().unwrap_or_default();
    let submitted_by_id = strokes.iter().filter_map(|stroke| stroke.get("id").and_then(Value::as_str).map(|id| (id, stroke))).collect::<HashMap<_, _>>();
    for old_stroke in &previous_strokes {
        let Some(id) = old_stroke.get("id").and_then(Value::as_str) else { continue; };
        let foreign = old_stroke.get("author_id").and_then(Value::as_str).and_then(|author| Uuid::parse_str(author).ok()).is_none_or(|author| author != current.id);
        if !foreign { continue; }
        match submitted_by_id.get(id) {
            Some(new_stroke) if *new_stroke == old_stroke => {}
            Some(_) if request.erase_foreign => {}
            Some(_) => return Err(ApiError::forbidden("You can only modify your own freeform strokes.")),
            None if request.erase_foreign => {}
            None => return Err(ApiError::forbidden("Use the eraser to remove a чужой freeform stroke.")),
        }
    }
    let previous_ids = previous_strokes.iter().filter_map(|stroke| stroke.get("id").and_then(Value::as_str)).collect::<HashSet<_>>();
    for stroke in strokes {
        let (Some(id), Some(author_id)) = (stroke.get("id").and_then(Value::as_str), stroke.get("author_id").and_then(Value::as_str)) else { continue; };
        if !previous_ids.contains(id) {
            if Uuid::parse_str(author_id).ok() != Some(current.id) && !request.erase_foreign { return Err(ApiError::forbidden("New freeform strokes must belong to the current user.")); }
        }
    }
    sqlx::query("INSERT INTO board_freeform_drawings (board_id, document) VALUES ($1, $2) ON CONFLICT (board_id) DO UPDATE SET document = EXCLUDED.document, updated_at = now()")
        .bind(board_id).bind(SqlJson(&request.document)).execute(database(&state)?).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(BoardFreeformDrawingResponse { document: request.document }))
}

async fn search_board_cards(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Query(query): Query<BoardCardSearchQuery>) -> ApiResult<BoardCardSearchResponse> {
    let needle = query.q.trim();
    if needle.is_empty() { return Ok(Json(BoardCardSearchResponse { card_ids: Vec::new() })); }
    if needle.chars().count() > 240 { return Err(ApiError::bad_request("Search query is too long.")); }
    let pool = database(&state)?;
    ensure_board_layout_access(pool, board_id, current.id).await?;
    let limit = query.limit.unwrap_or(250).clamp(1, 500);
    // Keep every searchable field in its own indexed EXISTS branch. Combining
    // comments and checklist rows into one aggregate would force a full-board
    // scan even when PostgreSQL already has a matching GIN index.
    let cards = sqlx::query_scalar::<_, Uuid>(
        "SELECT c.id
         FROM cards c
         INNER JOIN boards b ON b.id = c.board_id
         WHERE c.board_id = $1 AND c.archived_at IS NULL
           AND (
             EXISTS(SELECT 1 FROM users u WHERE u.id = $3 AND u.is_system_owner AND u.disabled_at IS NULL)
             OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $3 AND wm.role IN ('owner', 'full_access'))
             OR EXISTS(
               SELECT 1 FROM board_members bm
               INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id
               WHERE bm.board_id = b.id AND bm.user_id = $3
                 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END
                   >= CASE c.min_view_preset WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 0 END
             )
           )
           AND (
             to_tsvector('simple', concat_ws(' ', c.title, c.description)) @@ websearch_to_tsquery('simple', $2)
             OR EXISTS(SELECT 1 FROM comments cm WHERE cm.card_id = c.id AND to_tsvector('simple', cm.body) @@ websearch_to_tsquery('simple', $2))
             OR EXISTS(SELECT 1 FROM checklists cl WHERE cl.card_id = c.id AND to_tsvector('simple', cl.title) @@ websearch_to_tsquery('simple', $2))
             OR EXISTS(SELECT 1 FROM checklist_items ci WHERE ci.card_id = c.id AND to_tsvector('simple', concat_ws(' ', ci.title, ci.description)) @@ websearch_to_tsquery('simple', $2))
             OR EXISTS(SELECT 1 FROM card_labels cl JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = c.id AND to_tsvector('simple', l.name) @@ websearch_to_tsquery('simple', $2))
             OR EXISTS(SELECT 1 FROM card_assignees ca JOIN users u ON u.id = ca.user_id WHERE ca.card_id = c.id AND to_tsvector('simple', u.display_name) @@ websearch_to_tsquery('simple', $2))
             OR EXISTS(SELECT 1 FROM card_profile_roles cpr JOIN profile_roles pr ON pr.id = cpr.role_id WHERE cpr.card_id = c.id AND to_tsvector('simple', pr.name) @@ websearch_to_tsquery('simple', $2))
           )
         ORDER BY c.updated_at DESC, c.id DESC
         LIMIT $4",
    )
    .bind(board_id)
    .bind(needle)
    .bind(current.id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(BoardCardSearchResponse { card_ids: cards }))
}

async fn list_board_activity(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Query(query): Query<BoardActivityQuery>) -> ApiResult<BoardActivityPageResponse> {
    let pool = database(&state)?;
    ensure_board_layout_access(pool, board_id, current.id).await?;
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(40).clamp(10, 100);
    let offset = (page - 1).saturating_mul(per_page);
    // Each row represents one human-readable action. Repeated identical
    // actions on the same card during one minute become a single feed entry.
    // Activity is available to signed-in project members, but its rows must
    // follow the same card visibility thresholds as the board itself.
    let visible_to_viewer = "(EXISTS(SELECT 1 FROM users u WHERE u.id = $3 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $3 AND wm.role IN ('owner', 'full_access')) OR EXISTS(SELECT 1 FROM board_members bm INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = b.id AND bm.user_id = $3 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END >= CASE c.min_view_preset WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 0 END))";
    let grouped = format!("SELECT a.card_id, a.action, a.detail, a.actor_id, date_trunc('minute', a.created_at) AS minute_bucket FROM card_activity a INNER JOIN cards c ON c.id = a.card_id INNER JOIN boards b ON b.id = c.board_id WHERE c.board_id = $1 AND ($2::uuid IS NULL OR a.actor_id = $2) AND {visible_to_viewer} GROUP BY a.card_id, a.action, a.detail, a.actor_id, date_trunc('minute', a.created_at)");
    let total: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM ({grouped}) grouped"))
        .bind(board_id)
        .bind(query.user_id)
        .bind(current.id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
    let items = sqlx::query_as::<_, BoardActivityItemResponse>(&format!(
        "SELECT MIN(a.id::text) AS id, a.card_id, c.title AS card_title, a.action, a.detail, a.actor_id, \
                COALESCE(u.username, 'Deleted user') AS actor_name, \
                CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS actor_avatar_url, \
                MAX(a.created_at)::text AS created_at, COUNT(*)::bigint AS count \
         FROM card_activity a \
         INNER JOIN cards c ON c.id = a.card_id \
         INNER JOIN boards b ON b.id = c.board_id \
         LEFT JOIN users u ON u.id = a.actor_id \
         WHERE c.board_id = $1 AND ($2::uuid IS NULL OR a.actor_id = $2) AND {visible_to_viewer} \
         GROUP BY a.card_id, c.title, a.action, a.detail, a.actor_id, u.id, u.username, u.avatar_key, date_trunc('minute', a.created_at) \
         ORDER BY MAX(a.created_at) DESC, MIN(a.id::text) DESC LIMIT $4 OFFSET $5"
    ))
    .bind(board_id)
    .bind(query.user_id)
    .bind(current.id)
    .bind(per_page)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(BoardActivityPageResponse { items, page, per_page, total }))
}

async fn board_events(State(state): State<AppState>, viewer: Viewer, Path(board_id): Path<Uuid>) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let actor_id = viewer.0.map(|user| user.id);
    let can_view: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public'))",
    )
    .bind(board_id)
    .bind(actor_id)
    .fetch_one(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if !can_view { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let pool = database(&state)?.clone();
    let stream = BroadcastStream::new(state.events.subscribe()).then(move |message| {
        let pool = pool.clone();
        async move {
            if message.is_err() { return Ok(Event::default().event("refresh").data(board_id.to_string())); }
            let allowed = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 LEFT JOIN users u ON u.id = m.user_id WHERE b.id = $1 AND b.archived_at IS NULL AND (b.visibility = 'public' OR (m.user_id IS NOT NULL AND u.disabled_at IS NULL)))")
                .bind(board_id).bind(actor_id).fetch_one(&pool).await.unwrap_or(false);
            if allowed {
                Ok(Event::default().event("refresh").data(board_id.to_string()))
            } else {
                Ok(Event::default().event("access-revoked").data("workspace access was revoked"))
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

async fn update_board(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardRequest>) -> ApiResult<BoardSummary> {
    ensure_board_permission(database(&state)?, board_id, current.id, "manage_permissions").await?;
    let title = valid_text(&request.title, "title", 200)?;
    let board = sqlx::query_as::<_, BoardSummary>(
        "UPDATE boards b SET title = $1, updated_at = now() FROM board_members m WHERE b.id = $2 AND m.board_id = b.id AND m.user_id = $3 AND b.archived_at IS NULL RETURNING b.id, b.title, b.visibility::text AS visibility",
    )
    .bind(title)
    .bind(board_id)
    .bind(current.id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::forbidden("Only workspace owners and admins can rename a board."))?;
    let _ = state.events.send(());
    Ok(Json(board))
}

async fn delete_board(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let keys = sqlx::query_scalar::<_, String>("SELECT a.object_key FROM attachments a JOIN cards c ON c.id = a.card_id WHERE c.board_id = $1 AND a.object_key IS NOT NULL UNION ALL SELECT cb.object_key FROM card_backgrounds cb JOIN cards c ON c.id = cb.card_id WHERE c.board_id = $1 UNION ALL SELECT object_key FROM board_stickers WHERE board_id = $1")
        .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let result = sqlx::query("DELETE FROM boards WHERE id = $1 AND archived_at IS NULL")
        .bind(board_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    for key in keys { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_board_background(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardBackgroundRequest>) -> Result<StatusCode, ApiError> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let url = match request.background_image_url {
        Some(value) if !value.trim().is_empty() => {
            let value = value.trim();
            if value.len() > 2_000 || !(value.starts_with("https://") || value.starts_with("/v1/")) { return Err(ApiError::bad_request("Background must be an HTTPS image URL or an uploaded Flowboard file.")); }
            Some(value.to_owned())
        }
        _ => None,
    };
    let fit = match request.background_fit.as_deref() {
        Some("cover") | Some("contain") | Some("fill") => request.background_fit,
        Some(_) => return Err(ApiError::bad_request("Background fit must be cover, contain, or fill.")),
        None => None,
    };
    let position = match request.background_position.as_deref() {
        Some("top") | Some("center") | Some("bottom") => request.background_position,
        Some(_) => return Err(ApiError::bad_request("Background position must be top, center, or bottom.")),
        None => None,
    };
    let result = sqlx::query("UPDATE boards b SET background_image_url = $1, background_fit = COALESCE($2, b.background_fit), background_position = COALESCE($3, b.background_position), updated_at = now() FROM board_members m WHERE b.id = $4 AND m.board_id = b.id AND m.user_id = $5 AND b.archived_at IS NULL")
        .bind(url).bind(fit).bind(position).bind(board_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_board_background(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<BoardBackgroundUploadResponse> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Background upload form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Background image file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Background image field must be named file.")); }
    let original_name = field.file_name().unwrap_or("board-background").replace(['/', '\\'], "_");
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_default();
    if !matches!(media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp") {
        return Err(ApiError::bad_request("Board background must be a JPEG, PNG, GIF, or WebP image."));
    }
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Background image could not be read."))?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 { return Err(ApiError::bad_request("Board background must be between 1 byte and 50 MiB.")); }
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Board background image type is unsupported."))?;
    let object_key = format!("board-background-{}.{}", Uuid::new_v4(), extension);
    let path = state.upload_dir.join(&object_key);
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "board background write failed"); ApiError::storage() })?;
    let previous_key = sqlx::query_scalar::<_, Option<String>>("SELECT object_key FROM board_backgrounds WHERE board_id = $1")
        .bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?.flatten();
    // Keep the canonical file path in the database.  The response gets a unique
    // cache-buster so the uploader immediately sees the newly selected file.
    let url = format!("/v1/boards/{board_id}/background/file");
    let result = sqlx::query("INSERT INTO board_backgrounds (board_id, uploaded_by, object_key, original_name, media_type, byte_size) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (board_id) DO UPDATE SET uploaded_by = EXCLUDED.uploaded_by, object_key = EXCLUDED.object_key, original_name = EXCLUDED.original_name, media_type = EXCLUDED.media_type, byte_size = EXCLUDED.byte_size, created_at = now()")
        .bind(board_id).bind(current.id).bind(&object_key).bind(&original_name).bind(&media_type).bind(bytes.len() as i64).execute(pool).await;
    if let Err(error) = result { let _ = tokio::fs::remove_file(&path).await; return Err(ApiError::internal(error)); }
    sqlx::query("UPDATE boards SET background_image_url = $1, updated_at = now() WHERE id = $2")
        .bind(&url).bind(board_id).execute(pool).await.map_err(ApiError::internal)?;
    if let Some(previous_key) = previous_key { let _ = tokio::fs::remove_file(state.upload_dir.join(previous_key)).await; }
    let _ = state.events.send(());
    Ok(Json(BoardBackgroundUploadResponse { url: format!("{url}?v={}", Uuid::new_v4()) }))
}

async fn download_board_background(State(state): State<AppState>, current: Viewer, Path(board_id): Path<Uuid>) -> Result<Response, ApiError> {
    let actor_id = current.0.map(|user| user.id);
    let background = sqlx::query_as::<_, (String, String)>("SELECT bb.object_key, bb.media_type FROM board_backgrounds bb INNER JOIN boards b ON b.id = bb.board_id LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE bb.board_id = $1 AND b.archived_at IS NULL AND (b.visibility = 'public' OR m.user_id IS NOT NULL)")
        .bind(board_id).bind(actor_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "background_not_found", "Board background was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(background.0)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "background_not_found", "Board background file was not found.".to_owned()) }
        else { tracing::error!(?error, "board background read failed"); ApiError::storage() }
    })?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&background.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"))], bytes).into_response())
}

fn board_sticker_response(board_id: Uuid, sticker: BoardStickerRow) -> BoardStickerResponse {
    BoardStickerResponse {
        id: sticker.id,
        name: sticker.name,
        media_type: sticker.media_type,
        url: format!("/v1/boards/{board_id}/stickers/{}/content", sticker.id),
    }
}

async fn ensure_board_sticker_read_access(pool: &PgPool, board_id: Uuid, actor_id: Option<Uuid>) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (b.visibility = 'public' OR m.user_id IS NOT NULL OR EXISTS(SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))",
    )
    .bind(board_id)
    .bind(actor_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    if allowed { Ok(()) } else { Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Project was not found.".to_owned())) }
}

async fn list_board_stickers(State(state): State<AppState>, current: Viewer, Path(board_id): Path<Uuid>) -> ApiResult<Vec<BoardStickerResponse>> {
    let pool = database(&state)?;
    ensure_board_sticker_read_access(pool, board_id, current.0.map(|user| user.id)).await?;
    let stickers = sqlx::query_as::<_, BoardStickerRow>("SELECT id, name, media_type FROM board_stickers WHERE board_id = $1 ORDER BY created_at, id")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?
        .into_iter()
        .map(|sticker| board_sticker_response(board_id, sticker))
        .collect();
    Ok(Json(stickers))
}

async fn upload_board_sticker(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<BoardStickerResponse> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM board_stickers WHERE board_id = $1")
        .bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if count >= 100 { return Err(ApiError::bad_request("A board can contain at most 100 custom stickers.")); }
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Sticker upload form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Sticker image file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Sticker image field must be named file.")); }
    let original_name = field.file_name().unwrap_or("sticker").replace(['/', '\\'], "_");
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_default();
    if !matches!(media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp") { return Err(ApiError::bad_request("Sticker must be a JPEG, PNG, GIF, or WebP image.")); }
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Sticker image could not be read."))?;
    if bytes.is_empty() || bytes.len() > 5 * 1024 * 1024 { return Err(ApiError::bad_request("Sticker must be between 1 byte and 5 MiB.")); }
    let sticker_id = Uuid::new_v4();
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Sticker image type is unsupported."))?;
    let object_key = format!("stickers/{sticker_id}.{extension}");
    let path = state.upload_dir.join(&object_key);
    if let Some(parent) = path.parent() { tokio::fs::create_dir_all(parent).await.map_err(|_| ApiError::storage())?; }
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "board sticker write failed"); ApiError::storage() })?;
    let name = original_name.rsplit_once('.').map(|(name, _)| name).unwrap_or(&original_name).trim();
    let name = if name.is_empty() { "Sticker" } else { name };
    let sticker = BoardStickerRow { id: sticker_id, name: valid_text(name, "sticker name", 80)?.to_owned(), media_type: media_type.clone() };
    if let Err(error) = sqlx::query("INSERT INTO board_stickers (id, board_id, name, object_key, media_type, byte_size, uploaded_by) VALUES ($1, $2, $3, $4, $5, $6, $7)")
        .bind(sticker.id).bind(board_id).bind(&sticker.name).bind(&object_key).bind(&sticker.media_type).bind(bytes.len() as i64).bind(current.id)
        .execute(pool).await {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(ApiError::internal(error));
    }
    let _ = state.events.send(());
    Ok(Json(board_sticker_response(board_id, sticker)))
}

async fn delete_board_sticker(State(state): State<AppState>, current: CurrentUser, Path((board_id, sticker_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let reaction_key = format!("sticker:{sticker_id}");
    sqlx::query("DELETE FROM comment_reactions r USING comments c, cards card WHERE r.comment_id = c.id AND c.card_id = card.id AND card.board_id = $1 AND r.emoji = $2")
        .bind(board_id).bind(&reaction_key).execute(pool).await.map_err(ApiError::internal)?;
    let object_key = sqlx::query_scalar::<_, String>("DELETE FROM board_stickers WHERE id = $1 AND board_id = $2 RETURNING object_key")
        .bind(sticker_id).bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "sticker_not_found", "Sticker was not found.".to_owned()))?;
    let _ = tokio::fs::remove_file(state.upload_dir.join(object_key)).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn download_board_sticker(State(state): State<AppState>, current: Viewer, Path((board_id, sticker_id)): Path<(Uuid, Uuid)>) -> Result<Response, ApiError> {
    let pool = database(&state)?;
    ensure_board_sticker_read_access(pool, board_id, current.0.map(|user| user.id)).await?;
    let sticker = sqlx::query_as::<_, (String, String)>("SELECT object_key, media_type FROM board_stickers WHERE id = $1 AND board_id = $2")
        .bind(sticker_id).bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "sticker_not_found", "Sticker was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(sticker.0)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "sticker_not_found", "Sticker file was not found.".to_owned()) }
        else { tracing::error!(?error, "board sticker read failed"); ApiError::storage() }
    })?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&sticker.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"))], bytes).into_response())
}

async fn update_board_visibility(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardVisibilityRequest>) -> Result<StatusCode, ApiError> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let visibility = match request.visibility.as_str() { "public" => "public", "private" => "private", _ => return Err(ApiError::bad_request("Visibility must be public or private.")) };
    let result = sqlx::query("UPDATE boards b SET visibility = $1::board_visibility, updated_at = now() FROM board_members m WHERE b.id = $2 AND m.board_id = b.id AND m.user_id = $3 AND b.archived_at IS NULL")
        .bind(visibility).bind(board_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_discord_integrations(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<DiscordIntegrationResponse>> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let integrations = sqlx::query_as::<_, DiscordIntegrationResponse>(
        "SELECT id, name, default_list_id, created_at::text AS created_at, last_used_at::text AS last_used_at, NULL::text AS token FROM discord_integrations WHERE board_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC",
    )
    .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(integrations))
}

async fn create_discord_integration(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateDiscordIntegrationRequest>) -> ApiResult<DiscordIntegrationResponse> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let name = valid_text(&request.name, "name", 120)?;
    if let Some(default_list_id) = request.default_list_id {
        let belongs_to_board: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lists WHERE id = $1 AND board_id = $2)")
            .bind(default_list_id).bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !belongs_to_board { return Err(ApiError::bad_request("The default list must belong to this board.")); }
    }
    let token = format!("fb_discord_{}", new_token());
    let integration = sqlx::query_as::<_, DiscordIntegrationResponse>(
        "INSERT INTO discord_integrations (id, board_id, default_list_id, created_by, name, token_hash) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, name, default_list_id, created_at::text AS created_at, last_used_at::text AS last_used_at, NULL::text AS token",
    )
    .bind(Uuid::new_v4()).bind(board_id).bind(request.default_list_id).bind(current.id).bind(name).bind(token_hash(&token))
    .fetch_one(pool).await.map_err(ApiError::internal)?;
    Ok(Json(DiscordIntegrationResponse { token: Some(token), ..integration }))
}

async fn revoke_discord_integration(State(state): State<AppState>, current: CurrentUser, Path((board_id, integration_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_full_access(pool, board_id, current.id).await?;
    let result = sqlx::query("UPDATE discord_integrations SET revoked_at = now() WHERE id = $1 AND board_id = $2 AND revoked_at IS NULL")
        .bind(integration_id).bind(board_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "discord_integration_not_found", "Discord integration was not found.".to_owned())); }
    Ok(StatusCode::NO_CONTENT)
}

fn import_string(value: &Value, key: &str, max_len: usize) -> Option<String> {
    value.get(key)?.as_str().map(|text| text.trim().chars().take(max_len).collect::<String>()).filter(|text| !text.is_empty())
}

fn import_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    value?.as_str().and_then(|text| DateTime::parse_from_rfc3339(text).ok()).map(|time| time.with_timezone(&Utc))
}

fn trello_label_color(color: Option<&str>) -> String {
    let color = color.unwrap_or_default();
    if color.len() == 7 && color.starts_with('#') && color.chars().skip(1).all(|character| character.is_ascii_hexdigit()) { return color.to_owned(); }
    match color {
        "green" => "#2F9E6D", "yellow" => "#D5A72C", "orange" => "#D9771F", "red" => "#C94C4C",
        "purple" => "#8A5AC2", "blue" => "#3573C7", "sky" => "#3C9CC7", "lime" => "#668E2B",
        "pink" => "#B64B80", "black" => "#4D4D4D", _ => "#6975E8",
    }.to_owned()
}

async fn import_trello_board(State(state): State<AppState>, current: CurrentUser, Path(workspace_id): Path<Uuid>, Json(document): Json<Value>) -> ApiResult<ImportBoardResponse> {
    ensure_workspace_full_access(database(&state)?, workspace_id, current.id).await?;
    let board_title = import_string(&document, "name", 200).ok_or_else(|| ApiError::bad_request("Import file must contain a board name."))?;
    let source_lists = document.get("lists").and_then(Value::as_array).ok_or_else(|| ApiError::bad_request("Import file must contain a lists array."))?;
    let source_cards = document.get("cards").and_then(Value::as_array).ok_or_else(|| ApiError::bad_request("Import file must contain a cards array."))?;
    if source_lists.len() > 500 || source_cards.len() > 5_000 { return Err(ApiError::bad_request("Import is too large for one project.")); }
    let source_background = document.get("prefs").and_then(|prefs| prefs.get("backgroundImage")).and_then(Value::as_str).filter(|url| url.starts_with("https://") && url.len() <= 2_000).map(ToOwned::to_owned);
    let pool = database(&state)?;
    let current_username = sqlx::query_scalar::<_, String>("SELECT username FROM users WHERE id = $1").bind(current.id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let board_id = Uuid::new_v4();
    sqlx::query("INSERT INTO boards (id, workspace_id, title, created_by, background_image_url) VALUES ($1, $2, $3, $4, $5)")
        .bind(board_id).bind(workspace_id).bind(&board_title).bind(current.id).bind(source_background).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO board_members (board_id, user_id, role) VALUES ($1, $2, 'editor')")
        .bind(board_id).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    let mut label_ids = HashMap::new();
    for label in document.get("labels").and_then(Value::as_array).into_iter().flatten() {
        let (Some(source_id), Some(name)) = (import_string(label, "id", 128), import_string(label, "name", 60)) else { continue; };
        let label_id = Uuid::new_v4();
        sqlx::query("INSERT INTO labels (id, workspace_id, board_id, name, color) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (board_id, name) DO UPDATE SET color = EXCLUDED.color")
            .bind(label_id).bind(workspace_id).bind(board_id).bind(&name).bind(trello_label_color(label.get("color").and_then(Value::as_str))).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        let stored_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM labels WHERE board_id = $1 AND name = $2").bind(board_id).bind(&name).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
        label_ids.insert(source_id, stored_id);
    }
    let mut list_ids = HashMap::new(); let mut imported_lists = 0;
    for (index, list) in source_lists.iter().filter(|list| !list.get("closed").and_then(Value::as_bool).unwrap_or(false)).enumerate() {
        let Some(source_id) = import_string(list, "id", 128) else { continue; };
        let list_id = Uuid::new_v4();
        sqlx::query("INSERT INTO lists (id, board_id, title, position, grid_column, grid_row) VALUES ($1, $2, $3, $4, $5, 1)")
            .bind(list_id).bind(board_id).bind(import_string(list, "name", 200).unwrap_or_else(|| "Без названия".to_owned())).bind(((index + 1) * 1000) as i64).bind((index + 1) as i32).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        list_ids.insert(source_id, list_id); imported_lists += 1;
    }
    if list_ids.is_empty() { return Err(ApiError::bad_request("Import contains no active lists.")); }
    let mut card_ids = HashMap::new(); let mut card_positions: HashMap<Uuid, i64> = HashMap::new(); let mut imported_cards = 0;
    for card in source_cards.iter().filter(|card| !card.get("closed").and_then(Value::as_bool).unwrap_or(false)) {
        let (Some(source_id), Some(list_source_id)) = (import_string(card, "id", 128), import_string(card, "idList", 128)) else { continue; };
        let Some(&list_id) = list_ids.get(&list_source_id) else { continue; };
        let position = card_positions.entry(list_id).and_modify(|value| *value += 1000).or_insert(1000);
        let card_id = Uuid::new_v4();
        let priority = card.get("flowboardPriority").and_then(Value::as_i64).unwrap_or(0).clamp(0, 5) as i16;
        sqlx::query("INSERT INTO cards (id, board_id, list_id, title, description, position, due_at, completed_at, priority, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)")
            .bind(card_id).bind(board_id).bind(list_id).bind(import_string(card, "name", 500).unwrap_or_else(|| "Без названия".to_owned())).bind(import_string(card, "desc", 20_000).unwrap_or_default()).bind(*position).bind(import_timestamp(card.get("due"))).bind(import_timestamp(card.get("dateCompleted"))).bind(priority).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        for label_source_id in card.get("idLabels").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) { if let Some(label_id) = label_ids.get(label_source_id) { sqlx::query("INSERT INTO card_labels (card_id, label_id) VALUES ($1, $2) ON CONFLICT DO NOTHING").bind(card_id).bind(label_id).execute(&mut *transaction).await.map_err(ApiError::internal)?; } }
        let cover_source_id = card.get("idAttachmentCover").and_then(Value::as_str);
        let mut cover_attachment_id = None;
        for attachment in card.get("attachments").and_then(Value::as_array).into_iter().flatten() {
            let Some(url) = attachment.get("url").and_then(Value::as_str).filter(|url| url.starts_with("https://") && url.len() <= 2_000) else { continue; };
            let attachment_id = Uuid::new_v4();
            sqlx::query("INSERT INTO attachments (id, card_id, uploaded_by, object_key, original_name, media_type, byte_size, external_url) VALUES ($1, $2, NULL, NULL, $3, $4, $5, $6)")
                .bind(attachment_id).bind(card_id).bind(import_string(attachment, "name", 255).unwrap_or_else(|| "Imported attachment".to_owned())).bind(import_string(attachment, "mimeType", 120).unwrap_or_else(|| "application/octet-stream".to_owned())).bind(attachment.get("bytes").and_then(Value::as_i64).unwrap_or(0).max(0)).bind(url).execute(&mut *transaction).await.map_err(ApiError::internal)?;
            if attachment.get("id").and_then(Value::as_str) == cover_source_id { cover_attachment_id = Some(attachment_id); }
        }
        if let Some(attachment_id) = cover_attachment_id { sqlx::query("UPDATE cards SET cover_attachment_id = $1 WHERE id = $2").bind(attachment_id).bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?; }
        card_ids.insert(source_id, card_id); imported_cards += 1;
    }
    let mut checklist_positions: HashMap<Uuid, i64> = HashMap::new();
    for checklist in document.get("checklists").and_then(Value::as_array).into_iter().flatten() {
        let Some(card_source_id) = import_string(checklist, "idCard", 128) else { continue; }; let Some(&card_id) = card_ids.get(&card_source_id) else { continue; };
        let checklist_id = Uuid::new_v4();
        let position = checklist_positions.entry(card_id).and_modify(|value| *value += 1000).or_insert(1000);
        sqlx::query("INSERT INTO checklists (id, card_id, title, position) VALUES ($1, $2, $3, $4)").bind(checklist_id).bind(card_id).bind(import_string(checklist, "name", 200).unwrap_or_else(|| "Чек-лист".to_owned())).bind(*position).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        for (index, item) in checklist.get("checkItems").and_then(Value::as_array).into_iter().flatten().enumerate() { sqlx::query("INSERT INTO checklist_items (id, checklist_id, card_id, title, position, is_completed) VALUES ($1, $2, $3, $4, $5, $6)").bind(Uuid::new_v4()).bind(checklist_id).bind(card_id).bind(import_string(item, "name", 500).unwrap_or_else(|| "Пункт".to_owned())).bind(((index + 1) * 1000) as i64).bind(item.get("state").and_then(Value::as_str) == Some("complete")).execute(&mut *transaction).await.map_err(ApiError::internal)?; }
    }
    let mut imported_comments = 0;
    for action in document.get("actions").and_then(Value::as_array).into_iter().flatten() {
        let card_source_id = action.get("data").and_then(|data| data.get("card")).and_then(|card| card.get("id")).and_then(Value::as_str); let Some(card_source_id) = card_source_id else { continue; }; let Some(&card_id) = card_ids.get(card_source_id) else { continue; };
        let created_at = import_timestamp(action.get("date")).unwrap_or_else(Utc::now); let action_type = import_string(action, "type", 120).unwrap_or_else(|| "importedAction".to_owned()); let detail = action.get("data").and_then(|data| data.get("text")).and_then(Value::as_str).unwrap_or_default().chars().take(1000).collect::<String>(); let actor_id = action.get("memberCreator").and_then(|member| member.get("username")).and_then(Value::as_str).filter(|username| *username == current_username).map(|_| current.id);
        if action_type == "commentCard" && !detail.trim().is_empty() { sqlx::query("INSERT INTO comments (id, card_id, author_id, body, created_at) VALUES ($1, $2, $3, $4, $5)").bind(Uuid::new_v4()).bind(card_id).bind(actor_id).bind(detail.clone()).bind(created_at).execute(&mut *transaction).await.map_err(ApiError::internal)?; imported_comments += 1; }
        sqlx::query("INSERT INTO card_activity (id, card_id, actor_id, action, detail, created_at) VALUES ($1, $2, $3, $4, $5, $6)").bind(Uuid::new_v4()).bind(card_id).bind(actor_id).bind(action_type).bind(detail).bind(created_at).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?; let _ = state.events.send(());
    Ok(Json(ImportBoardResponse { id: board_id, title: board_title, imported_lists, imported_cards, imported_comments }))
}

async fn export_board(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Value> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?; let pool = database(&state)?;
    let board = sqlx::query_as::<_, BoardAccess>("SELECT id, workspace_id, title, background_image_url, background_fit, background_position, visibility::text AS visibility FROM boards WHERE id = $1 AND archived_at IS NULL").bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let lists = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id::text, 'name', title, 'closed', false, 'pos', position) ORDER BY position), '[]'::jsonb) FROM lists WHERE board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let labels = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id::text, 'name', name, 'color', color) ORDER BY name), '[]'::jsonb) FROM labels WHERE board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let cards = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', c.id::text, 'name', c.title, 'desc', c.description, 'flowboardPriority', c.priority, 'closed', c.archived_at IS NOT NULL, 'due', c.due_at, 'dateCompleted', c.completed_at, 'idList', c.list_id::text, 'idAttachmentCover', c.cover_attachment_id::text, 'idLabels', COALESCE((SELECT jsonb_agg(label_id::text) FROM card_labels WHERE card_id = c.id), '[]'::jsonb), 'attachments', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', a.id::text, 'name', a.original_name, 'mimeType', a.media_type, 'bytes', a.byte_size, 'url', COALESCE(a.external_url, '/v1/attachments/' || a.id::text || '/content'))) FROM attachments a WHERE a.card_id = c.id), '[]'::jsonb)) ORDER BY c.position), '[]'::jsonb) FROM cards c WHERE c.board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let checklists = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', cl.id::text, 'name', cl.title, 'idCard', cl.card_id::text, 'pos', cl.position, 'checkItems', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', ci.id::text, 'name', ci.title, 'pos', ci.position, 'state', CASE WHEN ci.is_completed THEN 'complete' ELSE 'incomplete' END) ORDER BY ci.position) FROM checklist_items ci WHERE ci.checklist_id = cl.id), '[]'::jsonb)) ORDER BY cl.position), '[]'::jsonb) FROM checklists cl INNER JOIN cards c ON c.id = cl.card_id WHERE c.board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let actions = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(payload ORDER BY created_at DESC), '[]'::jsonb) FROM (SELECT a.created_at, jsonb_build_object('id', a.id::text, 'type', a.action, 'date', a.created_at, 'data', jsonb_build_object('text', a.detail, 'card', jsonb_build_object('id', a.card_id::text)), 'memberCreator', CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('id', u.id::text, 'username', u.username) END) AS payload FROM card_activity a INNER JOIN cards c ON c.id = a.card_id LEFT JOIN users u ON u.id = a.actor_id WHERE c.board_id = $1 AND a.action <> 'Добавлен комментарий' UNION ALL SELECT cm.created_at, jsonb_build_object('id', cm.id::text, 'type', 'commentCard', 'date', cm.created_at, 'data', jsonb_build_object('text', cm.body, 'card', jsonb_build_object('id', cm.card_id::text)), 'memberCreator', CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('id', u.id::text, 'username', u.username) END) AS payload FROM comments cm INNER JOIN cards c ON c.id = cm.card_id LEFT JOIN users u ON u.id = cm.author_id WHERE c.board_id = $1) exported_actions").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    Ok(Json(json!({ "format": "flowboard-trello-compatible/v1", "name": board.title, "prefs": { "backgroundImage": board.background_image_url }, "lists": lists, "cards": cards, "labels": labels, "checklists": checklists, "actions": actions, "members": [] })))
}

async fn list_board_automations(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<BoardAutomationResponse>> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let automations = sqlx::query_as::<_, BoardAutomationResponse>(
        "SELECT a.id, a.name, (a.condition->>'list_id')::uuid AS list_id, l.title AS list_title, a.action_type, (a.action->>'priority')::smallint AS action_priority, a.enabled, a.created_at::text AS created_at FROM board_automations a INNER JOIN lists l ON l.id = (a.condition->>'list_id')::uuid WHERE a.board_id = $1 ORDER BY a.created_at DESC",
    ).bind(board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(automations))
}

async fn create_board_automation(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateBoardAutomationRequest>) -> ApiResult<BoardAutomationResponse> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let name = valid_text(&request.name, "name", 120)?;
    let action_type = match request.action_type.as_str() {
        "complete_card" | "reopen_card" | "set_priority" | "archive_card" => request.action_type,
        _ => return Err(ApiError::bad_request("Unsupported automation action.")),
    };
    let action_priority = match action_type.as_str() {
        "set_priority" => match request.action_priority {
            Some(value) if (0..=5).contains(&value) => Some(value),
            _ => return Err(ApiError::bad_request("Priority automation requires a priority from 0 to 5.")),
        },
        _ => None,
    };
    let automation = sqlx::query_as::<_, BoardAutomationResponse>(
        "INSERT INTO board_automations (id, board_id, name, trigger_type, condition, action_type, action, created_by) SELECT $1, $2, $3, 'card_moved', jsonb_build_object('list_id', $4::text), $5, jsonb_build_object('priority', $6), $7 WHERE EXISTS(SELECT 1 FROM lists WHERE id = $4 AND board_id = $2) RETURNING id, name, (condition->>'list_id')::uuid AS list_id, (SELECT title FROM lists WHERE id = (condition->>'list_id')::uuid) AS list_title, action_type, (action->>'priority')::smallint AS action_priority, enabled, created_at::text AS created_at",
    ).bind(Uuid::new_v4()).bind(board_id).bind(name).bind(request.list_id).bind(action_type).bind(action_priority).bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("The selected column is not part of this project."))?;
    let _ = state.events.send(());
    Ok(Json(automation))
}

async fn update_board_automation(State(state): State<AppState>, current: CurrentUser, Path((board_id, automation_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateBoardAutomationRequest>) -> ApiResult<BoardAutomationResponse> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let automation = sqlx::query_as::<_, BoardAutomationResponse>(
        "UPDATE board_automations a SET enabled = $1, updated_at = now() WHERE a.id = $2 AND a.board_id = $3 RETURNING a.id, a.name, (a.condition->>'list_id')::uuid AS list_id, (SELECT title FROM lists WHERE id = (a.condition->>'list_id')::uuid) AS list_title, a.action_type, (a.action->>'priority')::smallint AS action_priority, a.enabled, a.created_at::text AS created_at",
    ).bind(request.enabled).bind(automation_id).bind(board_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "automation_not_found", "Automation was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(automation))
}

async fn delete_board_automation(State(state): State<AppState>, current: CurrentUser, Path((board_id, automation_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    ensure_board_full_access(database(&state)?, board_id, current.id).await?;
    let deleted = sqlx::query("DELETE FROM board_automations WHERE id = $1 AND board_id = $2").bind(automation_id).bind(board_id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "automation_not_found", "Automation was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn run_card_move_automations(pool: &PgPool, board_id: Uuid, card_id: Uuid, target_list_id: Uuid, actor_id: Uuid) {
    let automations = match sqlx::query_as::<_, BoardAutomationExecution>("SELECT name, action_type, (action->>'priority')::smallint AS action_priority FROM board_automations WHERE board_id = $1 AND trigger_type = 'card_moved' AND enabled AND condition->>'list_id' = $2::text ORDER BY created_at")
        .bind(board_id).bind(target_list_id).fetch_all(pool).await { Ok(items) => items, Err(error) => { tracing::error!(?error, "automation lookup failed"); return; } };
    for automation in automations {
        let (changed, action, detail) = match automation.action_type.as_str() {
            "complete_card" if ensure_card_has_no_active_blockers(pool, card_id).await.is_ok() => {
                let changed = sqlx::query("UPDATE cards SET completed_at = now(), updated_at = now() WHERE id = $1 AND archived_at IS NULL AND completed_at IS NULL").bind(card_id).execute(pool).await.map(|result| result.rows_affected() > 0).unwrap_or(false);
                (changed, "Автоматизация завершила задачу", String::new())
            }
            "reopen_card" => {
                let changed = sqlx::query("UPDATE cards SET completed_at = NULL, updated_at = now() WHERE id = $1 AND archived_at IS NULL AND completed_at IS NOT NULL").bind(card_id).execute(pool).await.map(|result| result.rows_affected() > 0).unwrap_or(false);
                (changed, "Автоматизация открыла задачу", String::new())
            }
            "set_priority" if automation.action_priority.is_some() => {
                let priority = automation.action_priority.unwrap();
                let changed = sqlx::query("UPDATE cards SET priority = $1, updated_at = now() WHERE id = $2 AND archived_at IS NULL AND priority IS DISTINCT FROM $1").bind(priority).bind(card_id).execute(pool).await.map(|result| result.rows_affected() > 0).unwrap_or(false);
                (changed, "Автоматизация изменила приоритет", format!("Приоритет: {priority}/5"))
            }
            "archive_card" => {
                let changed = sqlx::query("UPDATE cards SET archived_at = now(), updated_at = now() WHERE id = $1 AND archived_at IS NULL").bind(card_id).execute(pool).await.map(|result| result.rows_affected() > 0).unwrap_or(false);
                (changed, "Автоматизация архивировала задачу", String::new())
            }
            _ => (false, "", String::new()),
        };
        if changed { record_card_activity(pool, card_id, actor_id, action, &format!("{}{}", automation.name, if detail.is_empty() { String::new() } else { format!(" · {detail}") })).await; }
    }
}

async fn create_list(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateListRequest>) -> ApiResult<ListResponse> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "create_lists").await?;
    let title = valid_text(&request.title, "title", 200)?;
    // The standard board is one horizontal sequence.  `below_list_id` is
    // intentionally ignored for compatibility with older clients; freeform
    // placement lives in each user's personal layout instead.
    let grid_column = sqlx::query_scalar::<_, i32>("SELECT COALESCE(MAX(grid_column), 0) + 1 FROM lists WHERE board_id = $1")
        .bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let list = sqlx::query_as::<_, ListResponse>(
        "INSERT INTO lists (id, board_id, title, position, grid_column, grid_row) VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) FROM lists WHERE board_id = $2), 0) + 1000, $4, 1) RETURNING id, title, grid_column, grid_row, card_limit, is_public",
    )
    .bind(Uuid::new_v4()).bind(board_id).bind(title).bind(grid_column)
    .fetch_one(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(list))
}

async fn create_label(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateLabelRequest>) -> ApiResult<LabelResponse> {
    ensure_board_permission(database(&state)?, board_id, current.id, "create_labels").await?;
    let name = valid_text(&request.name, "name", 60)?;
    let color = valid_label_color(&request.color)?;
    let icon_shape = valid_profile_role_shape(&request.icon_shape)?;
    let icon_color = valid_label_color(&request.icon_color)?;
    let label = sqlx::query_as::<_, LabelResponse>(
        "INSERT INTO labels (id, workspace_id, board_id, name, color, icon_shape, icon_color) SELECT $1, b.workspace_id, b.id, $2, $3, $4, $5 FROM boards b WHERE b.id = $6 AND b.archived_at IS NULL ON CONFLICT (board_id, name) DO UPDATE SET color = EXCLUDED.color, icon_shape = EXCLUDED.icon_shape, icon_color = EXCLUDED.icon_color RETURNING id, name, color, icon_shape, icon_color",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(color)
    .bind(icon_shape)
    .bind(icon_color)
    .bind(board_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(label))
}

async fn update_label(State(state): State<AppState>, current: CurrentUser, Path(label_id): Path<Uuid>, Json(request): Json<UpdateLabelRequest>) -> ApiResult<LabelResponse> {
    let pool = database(&state)?;
    let current_label = sqlx::query_as::<_, (Uuid, String, String, String, String)>("SELECT board_id, name, color, icon_shape, icon_color FROM labels WHERE id = $1")
        .bind(label_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "label_not_found", "Label was not found.".to_owned()))?;
    ensure_board_permission(pool, current_label.0, current.id, "create_labels").await?;
    let name = match request.name { Some(value) => valid_text(&value, "name", 60)?.to_owned(), None => current_label.1 };
    let color = match request.color { Some(value) => valid_label_color(&value)?, None => current_label.2 };
    let icon_shape = match request.icon_shape { Some(value) => valid_profile_role_shape(&value)?.to_owned(), None => current_label.3 };
    let icon_color = match request.icon_color { Some(value) => valid_label_color(&value)?, None => current_label.4 };
    let duplicate_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM labels WHERE board_id = $1 AND name = $2 AND id <> $3)")
        .bind(current_label.0).bind(&name).bind(label_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if duplicate_exists { return Err(ApiError::bad_request("A label with this name already exists on the board.")); }
    let label = sqlx::query_as::<_, LabelResponse>("UPDATE labels SET name = $1, color = $2, icon_shape = $3, icon_color = $4 WHERE id = $5 RETURNING id, name, color, icon_shape, icon_color")
        .bind(name).bind(color).bind(icon_shape).bind(icon_color).bind(label_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(label))
}

async fn delete_label(State(state): State<AppState>, current: CurrentUser, Path(label_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM labels WHERE id = $1")
        .bind(label_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "label_not_found", "Label was not found.".to_owned()))?;
    ensure_board_permission(pool, board_id, current.id, "delete_labels").await?;
    let deleted = sqlx::query("DELETE FROM labels WHERE id = $1").bind(label_id).execute(pool).await.map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "label_not_found", "Label was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn create_milestone(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateMilestoneRequest>) -> ApiResult<MilestoneResponse> {
    ensure_board_permission(database(&state)?, board_id, current.id, "create_labels").await?;
    let name = valid_text(&request.name, "name", 120)?;
    let description = request.description.trim().chars().take(2_000).collect::<String>();
    let color = valid_label_color(request.color.as_deref().unwrap_or("#6ea8fe"))?;
    let milestone = sqlx::query_as::<_, MilestoneResponse>(
        "INSERT INTO milestones (id, board_id, name, description, color) SELECT $1, b.id, $2, $3, $4 FROM boards b WHERE b.id = $5 AND b.archived_at IS NULL ON CONFLICT (board_id, name) DO UPDATE SET description = EXCLUDED.description, color = EXCLUDED.color, updated_at = now() RETURNING id, name, description, color, target_date::text AS target_date",
    )
    .bind(Uuid::new_v4()).bind(name).bind(description).bind(color).bind(board_id)
    .fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(milestone))
}

async fn update_milestone(State(state): State<AppState>, current: CurrentUser, Path(milestone_id): Path<Uuid>, Json(request): Json<UpdateMilestoneRequest>) -> ApiResult<MilestoneResponse> {
    let pool = database(&state)?;
    let current_milestone = sqlx::query_as::<_, (Uuid, String, String, String)>("SELECT board_id, name, description, color FROM milestones WHERE id = $1")
        .bind(milestone_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "milestone_not_found", "Milestone was not found.".to_owned()))?;
    ensure_board_permission(pool, current_milestone.0, current.id, "create_labels").await?;
    let name = request.name.as_deref().map(|value| valid_text(value, "name", 120).map(ToOwned::to_owned)).transpose()?.unwrap_or(current_milestone.1);
    let description = request.description.unwrap_or(current_milestone.2).trim().chars().take(2_000).collect::<String>();
    let color = request.color.as_deref().map(valid_label_color).transpose()?.unwrap_or(current_milestone.3);
    let milestone = sqlx::query_as::<_, MilestoneResponse>("UPDATE milestones SET name = $1, description = $2, color = $3, updated_at = now() WHERE id = $4 RETURNING id, name, description, color, target_date::text AS target_date")
        .bind(name).bind(description).bind(color).bind(milestone_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(milestone))
}

async fn delete_milestone(State(state): State<AppState>, current: CurrentUser, Path(milestone_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM milestones WHERE id = $1")
        .bind(milestone_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "milestone_not_found", "Milestone was not found.".to_owned()))?;
    ensure_board_permission(pool, board_id, current.id, "delete_labels").await?;
    sqlx::query("DELETE FROM milestones WHERE id = $1").bind(milestone_id).execute(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_list(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>, Json(request): Json<UpdateListRequest>) -> ApiResult<ListResponse> {
    ensure_list_permission(database(&state)?, list_id, current.id, "create_lists").await?;
    let actor_id = current.id;
    let existing = sqlx::query_as::<_, (String, i32, bool)>("SELECT title, card_limit, is_public FROM lists WHERE id = $1")
        .bind(list_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    let title = request.title.as_deref().map(|value| valid_text(value, "title", 200).map(ToOwned::to_owned)).transpose()?.unwrap_or(existing.0);
    let card_limit = request.card_limit.unwrap_or(existing.1);
    let is_public = request.is_public.unwrap_or(existing.2);
    if !(0..=10_000).contains(&card_limit) { return Err(ApiError::bad_request("Card limit must be between 0 and 10000.")); }
    if request.is_public.is_some() { ensure_list_full_access(database(&state)?, list_id, current.id).await?; }
    let list = sqlx::query_as::<_, ListResponse>(
        "UPDATE lists l SET title = $1, card_limit = $2, is_public = $3, updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE l.id = $4 AND l.board_id = b.id AND m.user_id = $5 RETURNING l.id, l.title, l.grid_column, l.grid_row, l.card_limit, l.is_public",
    )
    .bind(title)
    .bind(card_limit)
    .bind(is_public)
    .bind(list_id)
    .bind(actor_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(list))
}

async fn move_list(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>, Json(request): Json<MoveListRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_list_permission(pool, list_id, current.id, "create_lists").await?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let list = sqlx::query_as::<_, (Uuid,)>("SELECT board_id FROM lists WHERE id = $1 FOR UPDATE")
        .bind(list_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    let before_list_id = request.before_list_id.or(request.below_list_id);
    if before_list_id == Some(list_id) { transaction.commit().await.map_err(ApiError::internal)?; return Ok(StatusCode::NO_CONTENT); }
    match before_list_id {
        Some(before_list_id) => {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lists WHERE id = $1 AND board_id = $2)")
                .bind(before_list_id).bind(list.0).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
            if !exists { return Err(ApiError::bad_request("Target list is unavailable.")); }
            sqlx::query(
                "WITH target AS (SELECT position FROM lists WHERE id = $2), previous AS (SELECT position FROM lists WHERE board_id = $1 AND id <> $3 AND position < (SELECT position FROM target) ORDER BY position DESC LIMIT 1) UPDATE lists SET position = COALESCE(((SELECT position FROM previous) + (SELECT position FROM target)) / 2, (SELECT position FROM target) - 1000), updated_at = now() WHERE id = $3",
            )
            .bind(list.0).bind(before_list_id).bind(list_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        }
        None => {
            sqlx::query("UPDATE lists SET position = COALESCE((SELECT MAX(position) FROM lists WHERE board_id = $1 AND id <> $2), 0) + 1000, updated_at = now() WHERE id = $2")
                .bind(list.0).bind(list_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        }
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_list(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    ensure_list_permission(database(&state)?, list_id, current.id, "delete_lists").await?;
    let actor_id = current.id;
    let pool = database(&state)?;
    let list = sqlx::query_as::<_, (Uuid,)>(
        "SELECT l.id FROM lists l INNER JOIN boards b ON b.id = l.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE l.id = $1 AND m.user_id = $2",
    )
    .bind(list_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    let archived_attachment_keys = sqlx::query_scalar::<_, Option<String>>("SELECT a.object_key FROM attachments a INNER JOIN cards c ON c.id = a.card_id WHERE c.list_id = $1 AND c.archived_at IS NOT NULL UNION ALL SELECT cb.object_key FROM card_backgrounds cb INNER JOIN cards c ON c.id = cb.card_id WHERE c.list_id = $1 AND c.archived_at IS NOT NULL")
        .bind(list.0).fetch_all(pool).await.map_err(ApiError::internal)?;
    let result = sqlx::query("DELETE FROM lists l WHERE l.id = $1 AND NOT EXISTS (SELECT 1 FROM cards c WHERE c.list_id = l.id AND c.archived_at IS NULL)")
        .bind(list.0)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 {
        let card_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE list_id = $1 AND archived_at IS NULL")
            .bind(list.0)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
        return Err(ApiError(StatusCode::CONFLICT, "list_not_empty", format!("Move or archive all {card_count} active cards before deleting this list.")));
    }
    for object_key in archived_attachment_keys.into_iter().flatten() {
        if let Err(error) = tokio::fs::remove_file(state.upload_dir.join(object_key)).await {
            if error.kind() != std::io::ErrorKind::NotFound { tracing::error!(?error, "archived attachment cleanup after list deletion failed"); }
        }
    }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_card_access(pool: &PgPool, card_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    let required = sqlx::query_scalar::<_, String>("SELECT min_view_preset FROM cards WHERE id = $1 AND archived_at IS NULL")
        .bind(card_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::forbidden("This card is not available to this account."))?;
    if actor_meets_card_preset(pool, card_id, user_id, &required).await? { Ok(()) }
    else { Err(ApiError::forbidden("This card is not available to this account.")) }
}

async fn ensure_card_public_read(pool: &PgPool, card_id: Uuid, user_id: Option<Uuid>) -> Result<(), ApiError> {
    let card = sqlx::query_as::<_, (bool, bool, String, String)>(
        "SELECT c.is_public, l.is_public, c.min_view_preset, b.visibility::text FROM cards c INNER JOIN lists l ON l.id = c.list_id INNER JOIN boards b ON b.id = c.board_id WHERE c.id = $1 AND b.archived_at IS NULL",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    if card.0 && card.1 && card.3 == "public" { return Ok(()); }
    if let Some(actor_id) = user_id {
        if actor_meets_card_preset(pool, card_id, actor_id, &card.2).await? { return Ok(()); }
    }
    Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))
}

async fn record_card_activity(pool: &PgPool, card_id: Uuid, actor_id: Uuid, action: &str, detail: &str) {
    if let Err(error) = sqlx::query("INSERT INTO card_activity (id, card_id, actor_id, action, detail) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::new_v4())
        .bind(card_id)
        .bind(actor_id)
        .bind(action)
        .bind(detail)
        .execute(pool)
        .await
    {
        tracing::error!(?error, card_id = %card_id, "card activity insert failed");
    }
    notify_card_watchers(pool, card_id, Some(actor_id), action, detail).await;
}

// Text fields are autosaved. A person pausing to think must not turn one edit
// into a wall of identical console entries (or watcher notifications). Fold
// consecutive text updates from the same person into the latest activity for
// ten minutes; another card action starts a new, meaningful history entry.
async fn record_card_edit_activity(pool: &PgPool, card_id: Uuid, actor_id: Uuid, detail: &str) {
    match sqlx::query(
        "UPDATE card_activity SET detail = $3 \
         WHERE id = (SELECT id FROM card_activity WHERE card_id = $1 ORDER BY created_at DESC, id DESC LIMIT 1) \
           AND actor_id = $2 \
           AND action = 'Изменена задача' \
           AND created_at > now() - interval '10 minutes'",
    )
    .bind(card_id)
    .bind(actor_id)
    .bind(detail)
    .execute(pool)
    .await
    {
        Ok(result) if result.rows_affected() > 0 => {}
        Ok(_) => record_card_activity(pool, card_id, actor_id, "Изменена задача", detail).await,
        Err(error) => {
            tracing::error!(?error, card_id = %card_id, "card edit activity coalescing failed");
            record_card_activity(pool, card_id, actor_id, "Изменена задача", detail).await;
        }
    }
}

async fn record_external_card_activity(pool: &PgPool, card_id: Uuid, action: &str, detail: &str) {
    if let Err(error) = sqlx::query("INSERT INTO card_activity (id, card_id, actor_id, action, detail) VALUES ($1, $2, NULL, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(card_id)
        .bind(action)
        .bind(detail)
        .execute(pool)
        .await
    {
        tracing::error!(?error, card_id = %card_id, "external card activity insert failed");
    }
    notify_card_watchers(pool, card_id, None, action, detail).await;
}

async fn notify_card_watchers(pool: &PgPool, card_id: Uuid, actor_id: Option<Uuid>, action: &str, detail: &str) {
    let watchers = sqlx::query_scalar::<_, Uuid>(
        "SELECT cw.user_id FROM card_watchers cw INNER JOIN cards c ON c.id = cw.card_id INNER JOIN boards b ON b.id = c.board_id WHERE cw.card_id = $1 AND c.archived_at IS NULL AND b.archived_at IS NULL AND ($2::uuid IS NULL OR cw.user_id <> $2) AND (b.visibility = 'public' OR EXISTS(SELECT 1 FROM board_members bm WHERE bm.board_id = b.id AND bm.user_id = cw.user_id) OR EXISTS(SELECT 1 FROM users u WHERE u.id = cw.user_id AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = cw.user_id AND wm.role IN ('owner', 'full_access')))"
    )
    .bind(card_id)
    .bind(actor_id)
    .fetch_all(pool)
    .await;
    let watchers = match watchers {
        Ok(watchers) => watchers,
        Err(error) => { tracing::error!(?error, card_id = %card_id, "card watcher lookup failed"); return; }
    };
    for user_id in watchers {
        if let Err(error) = sqlx::query("INSERT INTO card_notifications (id, user_id, card_id, actor_id, action, detail) VALUES ($1, $2, $3, $4, $5, $6)")
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(card_id)
            .bind(actor_id)
            .bind(action)
            .bind(detail)
            .execute(pool)
            .await
        {
            tracing::error!(?error, card_id = %card_id, user_id = %user_id, "card notification insert failed");
        }
    }
}

async fn notify_card_reviewers(pool: &PgPool, card_id: Uuid, actor_id: Uuid, reviewer_ids: &[Uuid]) {
    for user_id in reviewer_ids.iter().copied().filter(|user_id| *user_id != actor_id) {
        let result = sqlx::query(
            "INSERT INTO card_notifications (id, user_id, card_id, actor_id, action, detail, source_kind, source_id) \
             VALUES ($1, $2, $3, $4, 'Нужна ваша проверка', 'Вас назначили проверяющим', 'review_request', $3) \
             ON CONFLICT (user_id, source_kind, source_id) WHERE source_kind IS NOT NULL AND source_id IS NOT NULL \
             DO UPDATE SET actor_id = EXCLUDED.actor_id, action = EXCLUDED.action, detail = EXCLUDED.detail, read_at = NULL, created_at = now()",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(card_id)
        .bind(actor_id)
        .execute(pool)
        .await;
        if let Err(error) = result {
            tracing::error!(?error, card_id = %card_id, user_id = %user_id, "reviewer notification insert failed");
        }
    }
}

async fn clear_card_review_request_notifications(pool: &PgPool, card_id: Uuid) {
    if let Err(error) = sqlx::query("DELETE FROM card_notifications WHERE card_id = $1 AND source_kind = 'review_request'")
        .bind(card_id)
        .execute(pool)
        .await
    {
        tracing::error!(?error, card_id = %card_id, "review request notification cleanup failed");
    }
}

async fn notify_review_requester(pool: &PgPool, card_id: Uuid, requester_id: Uuid, actor_id: Uuid, status: &str, detail: &str) {
    if requester_id == actor_id { return; }
    let action = match status {
        "approved" => "Проверка одобрена",
        "changes_requested" => "По проверке нужны правки",
        "rejected" => "Проверка отклонена",
        _ => return,
    };
    if let Err(error) = sqlx::query(
        "INSERT INTO card_notifications (id, user_id, card_id, actor_id, action, detail, source_kind, source_id) \
         VALUES ($1, $2, $3, $4, $5, $6, 'review_result', $3) \
         ON CONFLICT (user_id, source_kind, source_id) WHERE source_kind IS NOT NULL AND source_id IS NOT NULL \
         DO UPDATE SET actor_id = EXCLUDED.actor_id, action = EXCLUDED.action, detail = EXCLUDED.detail, read_at = NULL, created_at = now()",
    )
    .bind(Uuid::new_v4()).bind(requester_id).bind(card_id).bind(actor_id).bind(action).bind(detail)
    .execute(pool).await {
        tracing::error!(?error, card_id = %card_id, requester_id = %requester_id, "review requester notification insert failed");
    }
}

async fn load_card_comments(pool: &PgPool, card_id: Uuid, current_user_id: Option<Uuid>) -> Result<Vec<CommentResponse>, ApiError> {
    let rows = sqlx::query_as::<_, CommentRow>(
        "SELECT c.id, c.body, c.author_id, COALESCE(u.username, c.external_author_name, 'Deleted user') AS author_name, COALESCE(CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END, c.external_author_avatar_url) AS author_avatar_url, (SELECT pr.color FROM user_profile_roles upr INNER JOIN profile_roles pr ON pr.id = upr.role_id WHERE upr.user_id = c.author_id ORDER BY pr.name, pr.id LIMIT 1) AS author_role_color, c.parent_comment_id, c.created_at::text AS created_at, c.edited_at::text AS edited_at FROM comments c LEFT JOIN users u ON u.id = c.author_id WHERE c.card_id = $1 ORDER BY c.created_at DESC, c.id DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let mut comments = comment_responses(pool, rows, current_user_id).await?;
    load_comment_attachments(pool, card_id, &mut comments).await?;
    Ok(comments)
}

async fn comment_responses(pool: &PgPool, rows: Vec<CommentRow>, current_user_id: Option<Uuid>) -> Result<Vec<CommentResponse>, ApiError> {
    let mut comments: Vec<CommentResponse> = rows.into_iter().map(|row| CommentResponse {
        id: row.id, body: row.body, author_id: row.author_id, author_name: row.author_name, author_avatar_url: row.author_avatar_url, author_role_color: row.author_role_color, parent_comment_id: row.parent_comment_id,
        created_at: row.created_at, edited_at: row.edited_at, is_unread: false, has_unread_thread: false, reactions: vec![], attachments: vec![],
    }).collect();
    if comments.is_empty() { return Ok(comments); }

    let comment_ids: Vec<Uuid> = comments.iter().map(|comment| comment.id).collect();
    let reactions = sqlx::query_as::<_, CommentReactionRow>(
        "SELECT comment_id, emoji, COUNT(*)::bigint AS count, COALESCE(BOOL_OR(user_id = $2), false) AS reacted FROM comment_reactions WHERE comment_id = ANY($1) GROUP BY comment_id, emoji ORDER BY emoji",
    )
    .bind(&comment_ids)
    .bind(current_user_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    for comment in &mut comments {
        comment.reactions = reactions.iter().filter(|reaction| reaction.comment_id == comment.id).map(|reaction| CommentReactionResponse {
            emoji: reaction.emoji.clone(), count: reaction.count, reacted: reaction.reacted,
        }).collect();
    }
    if let Some(user_id) = current_user_id {
        let unread_rows = sqlx::query_as::<_, (Uuid, bool, bool)>(
            "SELECT c.id, \
                (c.author_id IS DISTINCT FROM $2 AND c.created_at > COALESCE(read_state.read_at, viewer.created_at)) AS is_unread, \
                EXISTS(SELECT 1 FROM comments reply LEFT JOIN comment_read_states reply_read ON reply_read.comment_id = reply.id AND reply_read.user_id = $2 WHERE reply.parent_comment_id = c.id AND reply.author_id IS DISTINCT FROM $2 AND reply.created_at > COALESCE(reply_read.read_at, viewer.created_at)) AS has_unread_thread \
             FROM comments c CROSS JOIN users viewer LEFT JOIN comment_read_states read_state ON read_state.comment_id = c.id AND read_state.user_id = $2 \
             WHERE c.id = ANY($1) AND viewer.id = $2",
        )
        .bind(&comment_ids).bind(user_id).fetch_all(pool).await.map_err(ApiError::internal)?;
        for comment in &mut comments {
            if let Some((_, is_unread, has_unread_thread)) = unread_rows.iter().find(|(id, _, _)| *id == comment.id) {
                comment.is_unread = *is_unread;
                comment.has_unread_thread = *has_unread_thread;
            }
        }
    }
    Ok(comments)
}

fn attachment_ids_in_comment(body: &str) -> Vec<Uuid> {
    body.split("/v1/attachments/").skip(1)
        .filter_map(|suffix| suffix.split('/').next().and_then(|value| Uuid::parse_str(value).ok()))
        .collect::<HashSet<_>>().into_iter().collect()
}

async fn load_comment_attachments(pool: &PgPool, card_id: Uuid, comments: &mut [CommentResponse]) -> Result<(), ApiError> {
    let attachment_ids: Vec<Uuid> = comments.iter().flat_map(|comment| attachment_ids_in_comment(&comment.body)).collect::<HashSet<_>>().into_iter().collect();
    if attachment_ids.is_empty() { return Ok(()); }
    let attachments = sqlx::query_as::<_, CommentAttachmentResponse>(
        "SELECT id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS download_url FROM attachments WHERE card_id = $1 AND checklist_item_id IS NULL AND id = ANY($2)",
    )
    .bind(card_id).bind(&attachment_ids).fetch_all(pool).await.map_err(ApiError::internal)?;
    for comment in comments {
        let ids: HashSet<Uuid> = attachment_ids_in_comment(&comment.body).into_iter().collect();
        comment.attachments = attachments.iter().filter(|attachment| ids.contains(&attachment.id)).cloned().collect();
    }
    Ok(())
}

fn mentioned_usernames(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut names = HashSet::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'@' { index += 1; continue; }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len() && (bytes[end].is_ascii_lowercase() || bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit() || matches!(bytes[end], b'_' | b'-' | b'.')) { end += 1; }
        if (3..=32).contains(&(end - start)) {
            names.insert(value[start..end].to_ascii_lowercase());
        }
        index = end.max(start);
    }
    names.into_iter().collect()
}

fn mentioned_role_names(value: &str) -> Vec<String> {
    let mut roles = HashSet::new();
    let mut remainder = value;
    while let Some(start) = remainder.find("@{") {
        let after_start = &remainder[start + 2..];
        let Some(end) = after_start.find('}') else { break; };
        let name = after_start[..end].trim();
        if (1..=80).contains(&name.chars().count()) { roles.insert(name.to_lowercase()); }
        remainder = &after_start[end + 1..];
    }
    roles.into_iter().collect()
}

fn has_plain_role_mention(value: &str, role_name: &str) -> bool {
    let value = value.to_lowercase();
    let token = format!("@{}", role_name.trim().to_lowercase());
    if token.len() <= 1 { return false; }
    let mut from = 0;
    while let Some(found) = value[from..].find(&token) {
        let start = from + found;
        let end = start + token.len();
        let before_is_name = value[..start].chars().next_back().is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'));
        let after_is_name = value[end..].chars().next().is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'));
        if !before_is_name && !after_is_name { return true; }
        from = end;
    }
    false
}

async fn replace_card_mentions(pool: &PgPool, card_id: Uuid, actor_id: Uuid, source_kind: &str, source_id: Uuid, body: &str) -> Result<(), ApiError> {
    replace_card_mentions_with_roles(pool, card_id, Some(actor_id), source_kind, source_id, body, &[], &[]).await
}

async fn replace_card_mentions_with_roles(pool: &PgPool, card_id: Uuid, actor_id: Option<Uuid>, source_kind: &str, source_id: Uuid, body: &str, mentioned_role_ids: &[Uuid], mentioned_user_ids: &[Uuid]) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM card_mentions WHERE source_kind = $1 AND source_id = $2")
        .bind(source_kind).bind(source_id).execute(pool).await.map_err(ApiError::internal)?;
    let usernames = mentioned_usernames(body);
    let role_names = mentioned_role_names(body);
    let mut resolved_role_ids = mentioned_role_ids.iter().copied().collect::<HashSet<_>>();
    let resolved_user_ids = mentioned_user_ids.iter().copied().collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
    // The Discord bridge keeps the visible fallback as `@Role name` so it
    // remains readable if its cached role endpoint is unavailable. Local
    // Flowboard text intentionally continues to require `@{Role name}` to
    // avoid turning an ordinary @username into a role mention.
    if actor_id.is_none() {
        let board_roles = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT DISTINCT pr.id, pr.name FROM cards c \
             INNER JOIN board_members bm ON bm.board_id = c.board_id \
             INNER JOIN user_profile_roles upr ON upr.user_id = bm.user_id \
             INNER JOIN profile_roles pr ON pr.id = upr.role_id \
             WHERE c.id = $1",
        )
        .bind(card_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
        for (role_id, role_name) in board_roles {
            if has_plain_role_mention(body, &role_name) { resolved_role_ids.insert(role_id); }
        }
    }
    if usernames.is_empty() && role_names.is_empty() && resolved_role_ids.is_empty() && resolved_user_ids.is_empty() { return Ok(()); }
    let resolved_role_ids = resolved_role_ids.into_iter().collect::<Vec<_>>();
    let users = sqlx::query_scalar::<_, Uuid>("SELECT DISTINCT bm.user_id FROM cards c JOIN board_members bm ON bm.board_id = c.board_id JOIN users u ON u.id = bm.user_id LEFT JOIN user_profile_roles upr ON upr.user_id = u.id LEFT JOIN profile_roles pr ON pr.id = upr.role_id WHERE c.id = $1 AND ($2::uuid IS NULL OR bm.user_id <> $2) AND u.disabled_at IS NULL AND (lower(u.username) = ANY($3) OR lower(pr.name) = ANY($4) OR pr.id = ANY($5) OR bm.user_id = ANY($6))")
        .bind(card_id).bind(actor_id).bind(usernames).bind(role_names).bind(&resolved_role_ids).bind(&resolved_user_ids).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mention_detail = body.chars().take(220).collect::<String>();
    for user_id in users {
        sqlx::query("INSERT INTO card_mentions (id, card_id, user_id, source_kind, source_id) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (user_id, source_kind, source_id) DO UPDATE SET card_id = EXCLUDED.card_id, created_at = now(), read_at = NULL")
            .bind(Uuid::new_v4()).bind(card_id).bind(user_id).bind(source_kind).bind(source_id).execute(pool).await.map_err(ApiError::internal)?;
        let notification = match source_kind {
            "comment" => Some(("Вас упомянули в обсуждении", "comment_mention")),
            "checklist_item_description" => Some(("Вас упомянули в описании пункта", "checklist_item_mention")),
            _ => None,
        };
        if let Some((action, notification_source_kind)) = notification {
            sqlx::query(
                "INSERT INTO card_notifications (id, user_id, card_id, actor_id, action, detail, source_kind, source_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
                 ON CONFLICT (user_id, source_kind, source_id) WHERE source_kind IS NOT NULL AND source_id IS NOT NULL \
                 DO UPDATE SET card_id = EXCLUDED.card_id, actor_id = EXCLUDED.actor_id, action = EXCLUDED.action, detail = EXCLUDED.detail, read_at = NULL, created_at = now()",
            )
            .bind(Uuid::new_v4()).bind(user_id).bind(card_id).bind(actor_id).bind(action).bind(&mention_detail).bind(notification_source_kind).bind(source_id)
            .execute(pool).await.map_err(ApiError::internal)?;
        }
    }
    Ok(())
}

async fn mark_card_mentions_read(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    sqlx::query("UPDATE card_mentions SET read_at = now() WHERE card_id = $1 AND user_id = $2 AND read_at IS NULL")
        .bind(card_id).bind(current.id).execute(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_card_comments_read(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    sqlx::query(
        "INSERT INTO comment_read_states (comment_id, user_id, read_at) \
         SELECT id, $2, now() FROM comments WHERE card_id = $1 AND parent_comment_id IS NULL AND author_id IS DISTINCT FROM $2 \
         ON CONFLICT (comment_id, user_id) DO UPDATE SET read_at = EXCLUDED.read_at",
    )
    .bind(card_id).bind(current.id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_comment_thread_read(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
        .bind(comment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "comment_not_found", "Comment was not found.".to_owned()))?;
    ensure_card_access(pool, card_id, current.id).await?;
    sqlx::query(
        "INSERT INTO comment_read_states (comment_id, user_id, read_at) \
         SELECT id, $2, now() FROM comments WHERE card_id = $1 AND (id = $3 OR parent_comment_id = $3) AND author_id IS DISTINCT FROM $2 \
         ON CONFLICT (comment_id, user_id) DO UPDATE SET read_at = EXCLUDED.read_at",
    )
    .bind(card_id).bind(current.id).bind(comment_id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn watch_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<CardWatchResponse> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, Some(current.id)).await?;
    sqlx::query("INSERT INTO card_watchers (card_id, user_id) VALUES ($1, $2) ON CONFLICT (card_id, user_id) DO NOTHING")
        .bind(card_id).bind(current.id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(Json(CardWatchResponse { watching: true }))
}

async fn unwatch_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<CardWatchResponse> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, Some(current.id)).await?;
    sqlx::query("DELETE FROM card_watchers WHERE card_id = $1 AND user_id = $2")
        .bind(card_id).bind(current.id).execute(pool).await.map_err(ApiError::internal)?;
    Ok(Json(CardWatchResponse { watching: false }))
}

async fn list_notifications(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<CardNotificationResponse>> {
    let notifications = sqlx::query_as::<_, CardNotificationResponse>(
        "SELECT n.id, n.card_id, c.board_id, c.title AS card_title, b.title AS board_title, COALESCE(u.username, 'Deleted user') AS actor_name, n.action, n.detail, n.read_at IS NOT NULL AS is_read, n.created_at::text AS created_at, n.source_kind, n.source_id FROM card_notifications n INNER JOIN cards c ON c.id = n.card_id INNER JOIN boards b ON b.id = c.board_id LEFT JOIN users u ON u.id = n.actor_id WHERE n.user_id = $1 AND b.archived_at IS NULL AND (b.visibility = 'public' OR EXISTS(SELECT 1 FROM board_members bm WHERE bm.board_id = b.id AND bm.user_id = n.user_id) OR EXISTS(SELECT 1 FROM users owner WHERE owner.id = n.user_id AND owner.is_system_owner AND owner.disabled_at IS NULL) OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = n.user_id AND wm.role IN ('owner', 'full_access'))) ORDER BY n.created_at DESC LIMIT 80"
    )
    .bind(current.id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(notifications))
}

async fn mark_notification_read(State(state): State<AppState>, current: CurrentUser, Path(notification_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    sqlx::query("UPDATE card_notifications SET read_at = now() WHERE id = $1 AND user_id = $2 AND read_at IS NULL")
        .bind(notification_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_notification_unread(State(state): State<AppState>, current: CurrentUser, Path(notification_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    sqlx::query("UPDATE card_notifications SET read_at = NULL WHERE id = $1 AND user_id = $2")
        .bind(notification_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_all_notifications_read(State(state): State<AppState>, current: CurrentUser) -> Result<StatusCode, ApiError> {
    sqlx::query("UPDATE card_notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL")
        .bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_card_detail(State(state): State<AppState>, current: Viewer, Path(card_id): Path<Uuid>) -> ApiResult<CardDetail> {
    let pool = database(&state)?;
    let actor_id = current.0.map(|user| user.id);
    ensure_card_public_read(pool, card_id, actor_id).await?;
    let (cover_attachment_id, cover_mode, background_image_url) = sqlx::query_as::<_, (Option<Uuid>, String, Option<String>)>("SELECT cover_attachment_id, cover_mode, background_image_url FROM cards WHERE id = $1")
        .bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let checklist_rows = sqlx::query_as::<_, ChecklistRow>(
        "SELECT id, title FROM checklists WHERE card_id = $1 ORDER BY position, id",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let checklist_items = sqlx::query_as::<_, ChecklistItemRow>(
        "SELECT checklist_id, id, title, is_completed, description FROM checklist_items WHERE card_id = $1 ORDER BY position, id",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let checklist_item_attachments = sqlx::query_as::<_, ChecklistItemAttachmentRow>(
        "SELECT checklist_item_id, id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS url FROM attachments WHERE card_id = $1 AND checklist_item_id IS NOT NULL ORDER BY created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let checklists = checklist_rows.into_iter().map(|checklist| ChecklistResponse {
        id: checklist.id,
        title: checklist.title,
        items: checklist_items.iter().filter(|item| item.checklist_id == checklist.id).map(|item| ChecklistItemResponse {
            id: item.id,
            title: item.title.clone(),
            is_completed: item.is_completed,
            description: item.description.clone(),
            attachments: checklist_item_attachments.iter().filter(|attachment| attachment.checklist_item_id == item.id).map(|attachment| AttachmentResponse {
                id: attachment.id,
                original_name: attachment.original_name.clone(),
                media_type: attachment.media_type.clone(),
                byte_size: attachment.byte_size,
                url: attachment.url.clone(),
            }).collect(),
        }).collect(),
    }).collect();
    let public_board_id = if actor_id.is_none() {
        Some(sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM cards WHERE id = $1")
            .bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?)
    } else { None };
    let mut comments = load_card_comments(pool, card_id, actor_id).await?;
    if let Some(board_id) = public_board_id {
        for comment in &mut comments {
            if let Some(author_id) = comment.author_id {
                comment.author_avatar_url = Some(format!("/v1/public/boards/{board_id}/avatars/{author_id}"));
            } else if comment.author_avatar_url.is_some() {
                comment.author_avatar_url = Some(format!("/v1/comments/{}/avatar", comment.id));
            }
        }
    } else {
        for comment in &mut comments {
            if comment.author_id.is_none() && comment.author_avatar_url.is_some() {
                comment.author_avatar_url = Some(format!("/v1/comments/{}/avatar", comment.id));
            }
        }
    }
    let attachments = sqlx::query_as::<_, AttachmentResponse>(
        "SELECT id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS url FROM attachments WHERE card_id = $1 AND checklist_item_id IS NULL ORDER BY created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let external_urls = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, external_url FROM attachments WHERE card_id = $1 AND external_url IS NOT NULL",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    for comment in &mut comments {
        for (attachment_id, external_url) in &external_urls {
            let local_url = format!("/v1/attachments/{attachment_id}/content");
            comment.body = comment.body.replace(external_url, &local_url);
        }
    }
    let mut activity = sqlx::query_as::<_, CardActivityResponse>(
        "SELECT a.id, a.action, a.detail, a.actor_id, COALESCE(u.username, 'Deleted user') AS actor_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS actor_avatar_url, (SELECT pr.color FROM user_profile_roles upr INNER JOIN profile_roles pr ON pr.id = upr.role_id WHERE upr.user_id = a.actor_id ORDER BY pr.name, pr.id LIMIT 1) AS actor_role_color, a.created_at::text AS created_at FROM card_activity a LEFT JOIN users u ON u.id = a.actor_id WHERE a.card_id = $1 ORDER BY a.created_at DESC LIMIT 100",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    if let Some(board_id) = public_board_id {
        for item in &mut activity {
            if let Some(actor_id) = item.actor_id {
                item.actor_avatar_url = Some(format!("/v1/public/boards/{board_id}/avatars/{actor_id}"));
            }
        }
    }
    let unread_mention_source_ids = if let Some(actor_id) = actor_id {
        sqlx::query_scalar::<_, Uuid>("SELECT source_id FROM card_mentions WHERE card_id = $1 AND user_id = $2 AND read_at IS NULL")
            .bind(card_id).bind(actor_id).fetch_all(pool).await.map_err(ApiError::internal)?
    } else { vec![] };
    let watching = if let Some(actor_id) = actor_id {
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM card_watchers WHERE card_id = $1 AND user_id = $2)")
            .bind(card_id).bind(actor_id).fetch_one(pool).await.map_err(ApiError::internal)?
    } else { false };
    Ok(Json(CardDetail { checklists, comments, attachments, activity, cover_attachment_id, cover_mode, background_image_url, unread_mention_source_ids, watching }))
}

fn validate_diagram_document(document: &Value) -> Result<(), ApiError> {
    let bytes = serde_json::to_vec(document).map_err(|_| ApiError::bad_request("Diagram document is invalid."))?;
    if bytes.len() > 1024 * 1024 { return Err(ApiError::bad_request("Diagram must be smaller than 1 MiB.")); }
    let strokes = document.get("strokes").and_then(Value::as_array).ok_or_else(|| ApiError::bad_request("Diagram must contain a strokes array."))?;
    let elements = match document.get("elements") {
        Some(value) => Some(value.as_array().ok_or_else(|| ApiError::bad_request("Diagram elements must be an array."))?),
        None => None,
    };
    if strokes.len() + elements.map_or(0, Vec::len) > 500 { return Err(ApiError::bad_request("Diagram can contain at most 500 objects.")); }
    if strokes.iter().any(|stroke| {
        stroke.get("points").and_then(Value::as_array).is_none_or(|points| points.len() > 2_000 || points.iter().any(|point| !point.get("x").is_some_and(Value::is_number) || !point.get("y").is_some_and(Value::is_number)))
    }) {
        return Err(ApiError::bad_request("A diagram stroke is invalid or too large."));
    }
    if strokes.iter().any(|stroke| stroke.get("color").is_some_and(|color| !color.is_string()) || stroke.get("width").is_some_and(|width| !width.is_number())) {
        return Err(ApiError::bad_request("A diagram stroke style is invalid."));
    }
    if let Some(elements) = elements {
        for element in elements {
            let type_name = element.get("type").and_then(Value::as_str).ok_or_else(|| ApiError::bad_request("A diagram element has no type."))?;
            let number = |field: &str| element.get(field).is_some_and(Value::is_number);
            let style = || element.get("color").and_then(Value::as_str).is_some_and(|value| value.len() <= 32);
            let optional_style = |field: &str| element.get(field).is_none_or(|value| value.as_str().is_some_and(|color| color.len() <= 32));
            let optional_number = |field: &str| element.get(field).is_none_or(Value::is_number);
            let optional_text = |field: &str| element.get(field).is_none_or(|value| value.as_str().is_some_and(|text| text.len() <= 4_000));
            let optional_font = || element.get("fontFamily").is_none_or(|value| value.as_str().is_some_and(|font| font.len() <= 120));
            let optional_weight = || element.get("fontWeight").is_none_or(|value| value.as_str().is_some_and(|weight| matches!(weight, "normal" | "bold")));
            let valid = match type_name {
                "rectangle" | "ellipse" => number("x") && number("y") && number("width") && number("height") && number("lineWidth") && style()
                    && optional_style("fillColor") && optional_style("textColor") && optional_number("cornerRadius") && optional_number("rotation") && optional_number("fontSize") && optional_text("text") && optional_font() && optional_weight(),
                "arrow" => number("x") && number("y") && number("x2") && number("y2") && number("lineWidth") && style() && optional_number("rotation"),
                "text" => number("x") && number("y") && number("fontSize") && style()
                    && element.get("text").and_then(Value::as_str).is_some_and(|value| value.len() <= 4_000)
                    && element.get("fontFamily").and_then(Value::as_str).is_some_and(|value| value.len() <= 120)
                    && element.get("fontWeight").and_then(Value::as_str).is_some_and(|value| matches!(value, "normal" | "bold")) && optional_number("rotation"),
                "callout" => number("x") && number("y") && number("x2") && number("y2") && number("fontSize") && style()
                    && element.get("text").and_then(Value::as_str).is_some_and(|value| value.len() <= 4_000)
                    && element.get("fontFamily").and_then(Value::as_str).is_some_and(|value| value.len() <= 120)
                    && element.get("fontWeight").and_then(Value::as_str).is_some_and(|value| matches!(value, "normal" | "bold")) && optional_number("rotation"),
                "image" => number("x") && number("y") && number("width") && number("height")
                    && element.get("src").and_then(Value::as_str).is_some_and(|value| value.len() <= 240 && value.starts_with("/v1/attachments/") && value.ends_with("/content")) && optional_number("rotation"),
                "sticker" => number("x") && number("y") && number("size") && style() && element.get("fillColor").and_then(Value::as_str).is_some_and(|value| value.len() <= 32)
                    && element.get("icon").and_then(Value::as_str).is_some_and(|value| matches!(value, "?" | "!")) && optional_number("rotation"),
                _ => false,
            };
            if !valid { return Err(ApiError::bad_request("A diagram element is invalid.")); }
        }
    }
    Ok(())
}

fn diagram_items(document: &Value, key: &str) -> Result<Vec<Value>, ApiError> {
    match document.get(key) {
        Some(value) => value.as_array().cloned().ok_or_else(|| ApiError::bad_request("Diagram document is malformed.")),
        None if key == "elements" => Ok(Vec::new()),
        None => Err(ApiError::bad_request("Diagram document is malformed.")),
    }
}

fn diagram_item_id(item: &Value) -> Result<String, ApiError> {
    let id = item.get("id").and_then(Value::as_str).ok_or_else(|| ApiError::bad_request("Collaborative diagram objects need an id."))?;
    if id.is_empty() || id.len() > 96 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
        return Err(ApiError::bad_request("A collaborative diagram object id is invalid."));
    }
    Ok(id.to_owned())
}

fn diagram_contains_unidentified_objects(document: &Value) -> bool {
    ["strokes", "elements"].iter().any(|key| {
        document.get(*key).and_then(Value::as_array).is_some_and(|items| {
            items.iter().any(|item| item.get("id").and_then(Value::as_str).is_none())
        })
    })
}

fn merge_diagram_items(current: Vec<Value>, base: Vec<Value>, incoming: Vec<Value>) -> Result<Vec<Value>, ApiError> {
    let base_by_id = base.iter().map(|item| Ok((diagram_item_id(item)?, item))).collect::<Result<HashMap<_, _>, ApiError>>()?;
    let incoming_by_id = incoming.iter().map(|item| Ok((diagram_item_id(item)?, item))).collect::<Result<HashMap<_, _>, ApiError>>()?;
    let mut merged = current;
    let deleted = base_by_id.keys().filter(|id| !incoming_by_id.contains_key(*id)).cloned().collect::<HashSet<_>>();
    merged.retain(|item| diagram_item_id(item).map_or(true, |id| !deleted.contains(&id)));
    for item in incoming {
        let id = diagram_item_id(&item)?;
        let changed = base_by_id.get(&id).is_none_or(|previous| **previous != item);
        if !changed { continue; }
        if let Some(index) = merged.iter().position(|current_item| diagram_item_id(current_item).is_ok_and(|current_id| current_id == id)) { merged[index] = item; }
        else { merged.push(item); }
    }
    Ok(merged)
}

fn merge_diagram_documents(current: Value, base: &Value, incoming: &Value) -> Result<Value, ApiError> {
    validate_diagram_document(base)?;
    validate_diagram_document(incoming)?;
    let current = if diagram_contains_unidentified_objects(&current) && !diagram_contains_unidentified_objects(base) { base.clone() } else { current };
    if diagram_contains_unidentified_objects(&current) || diagram_contains_unidentified_objects(base) || diagram_contains_unidentified_objects(incoming) {
        return Err(ApiError::bad_request("Collaborative diagram objects need stable ids."));
    }
    Ok(json!({
        "strokes": merge_diagram_items(diagram_items(&current, "strokes")?, diagram_items(base, "strokes")?, diagram_items(incoming, "strokes")?)?,
        "elements": merge_diagram_items(diagram_items(&current, "elements")?, diagram_items(base, "elements")?, diagram_items(incoming, "elements")?)?,
    }))
}

async fn card_diagram_presence_snapshot(state: &AppState, card_id: Uuid, current_user_id: Uuid) -> ApiResult<Vec<DiagramPresenceEntry>> {
    let active = {
        let now = Instant::now();
        let mut cards = state.diagram_presence.lock().await;
        let card = cards.entry(card_id).or_default();
        card.retain(|_, presence| now.duration_since(presence.last_seen) <= Duration::from_secs(8));
        card.iter()
            .filter(|(user_id, _)| **user_id != current_user_id)
            .map(|(user_id, presence)| DiagramPresenceEntry { user_id: *user_id, username: presence.username.clone(), avatar_url: presence.avatar_url.clone(), x: presence.x, y: presence.y })
            .collect::<Vec<_>>()
    };
    Ok(Json(active))
}

async fn diagram_live_account(state: &AppState, current: CurrentUser) -> Result<FreeformLiveAccount, ApiError> {
    sqlx::query_as::<_, FreeformLiveAccount>("SELECT id, username, CASE WHEN avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || id::text END AS avatar_url FROM users WHERE id = $1 AND disabled_at IS NULL")
        .bind(current.id).fetch_optional(database(state)?).await.map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)
}

async fn record_diagram_cursor(state: &AppState, card_id: Uuid, current: CurrentUser, account: &FreeformLiveAccount, x: i32, y: i32) -> Result<DiagramLiveEvent, ApiError> {
    if !(0..=20_000).contains(&x) || !(0..=20_000).contains(&y) {
        return Err(ApiError::bad_request("Diagram cursor coordinates must be between 0 and 20000."));
    }
    state.diagram_presence.lock().await.entry(card_id).or_default().insert(current.id, DiagramCursorPresence { x, y, username: account.username.clone(), avatar_url: account.avatar_url.clone(), last_seen: Instant::now() });
    Ok(DiagramLiveEvent::Cursor(DiagramCursorEvent { event_type: "diagram_cursor", card_id, user_id: current.id, username: account.username.clone(), avatar_url: account.avatar_url.clone(), x, y }))
}

async fn record_diagram_object_lock(state: &AppState, card_id: Uuid, current: CurrentUser, account: &FreeformLiveAccount, object_id: String, active: bool) -> DiagramLiveEvent {
    let now = Instant::now();
    let mut locks = state.diagram_locks.lock().await;
    let card_locks = locks.entry(card_id).or_default();
    card_locks.retain(|_, lock| lock.expires_at > now);
    if !active {
        if let Some(lock) = card_locks.get(&object_id) {
            if lock.user_id != current.id {
                return DiagramLiveEvent::ObjectLock(DiagramObjectLockEvent { event_type: "diagram_object_lock", card_id, object_id, user_id: lock.user_id, username: lock.username.clone(), active: true, expires_in_ms: lock.expires_at.saturating_duration_since(now).as_millis() as u64 });
            }
        }
        card_locks.remove(&object_id);
        return DiagramLiveEvent::ObjectLock(DiagramObjectLockEvent { event_type: "diagram_object_lock", card_id, object_id, user_id: current.id, username: account.username.clone(), active: false, expires_in_ms: 0 });
    }
    if let Some(lock) = card_locks.get(&object_id) {
        if lock.user_id != current.id {
            return DiagramLiveEvent::ObjectLock(DiagramObjectLockEvent { event_type: "diagram_object_lock", card_id, object_id, user_id: lock.user_id, username: lock.username.clone(), active: true, expires_in_ms: lock.expires_at.saturating_duration_since(now).as_millis() as u64 });
        }
    }
    // A lock is deliberately short-lived, but must still survive a thoughtful
    // pause while somebody edits a text label. The browser renews it while an
    // interaction is active and releases it immediately on pointer-up/blur.
    card_locks.insert(object_id.clone(), DiagramObjectLock { user_id: current.id, username: account.username.clone(), expires_at: now + Duration::from_secs(6) });
    DiagramLiveEvent::ObjectLock(DiagramObjectLockEvent { event_type: "diagram_object_lock", card_id, object_id, user_id: current.id, username: account.username.clone(), active: true, expires_in_ms: 6_000 })
}

/// Rejects only modifications to objects another editor has actively locked.
/// Independent edits remain optimistic and merge as before.
async fn ensure_diagram_object_locks(state: &AppState, card_id: Uuid, current: CurrentUser, base: &Value, incoming: &Value) -> Result<(), ApiError> {
    let locked_by_others = {
        let now = Instant::now();
        let mut locks = state.diagram_locks.lock().await;
        let card_locks = locks.entry(card_id).or_default();
        card_locks.retain(|_, lock| lock.expires_at > now);
        card_locks.iter()
            .filter(|(_, lock)| lock.user_id != current.id)
            .map(|(object_id, lock)| (object_id.clone(), lock.username.clone()))
            .collect::<Vec<_>>()
    };
    if locked_by_others.is_empty() { return Ok(()); }

    for key in ["strokes", "elements"] {
        let base_items = diagram_items(base, key)?;
        let incoming_items = diagram_items(incoming, key)?;
        let base_by_id = base_items.iter().map(|item| Ok((diagram_item_id(item)?, item))).collect::<Result<HashMap<_, _>, ApiError>>()?;
        let incoming_by_id = incoming_items.iter().map(|item| Ok((diagram_item_id(item)?, item))).collect::<Result<HashMap<_, _>, ApiError>>()?;
        for (object_id, username) in &locked_by_others {
            if base_by_id.get(object_id) != incoming_by_id.get(object_id) {
                return Err(ApiError(StatusCode::CONFLICT, "diagram_object_locked", format!("Слой сейчас редактирует @{username}.")));
            }
        }
    }
    Ok(())
}

fn diagram_live_event_card_id(event: &DiagramLiveEvent) -> Uuid {
    match event { DiagramLiveEvent::Merge(event) => event.card_id, DiagramLiveEvent::Cursor(event) => event.card_id, DiagramLiveEvent::ObjectLock(event) => event.card_id, DiagramLiveEvent::NotesChanged(event) => event.card_id }
}

fn diagram_live_event_payload(event: &DiagramLiveEvent) -> Result<String, serde_json::Error> {
    match event { DiagramLiveEvent::Merge(event) => serde_json::to_string(event), DiagramLiveEvent::Cursor(event) => serde_json::to_string(event), DiagramLiveEvent::ObjectLock(event) => serde_json::to_string(event), DiagramLiveEvent::NotesChanged(event) => serde_json::to_string(event) }
}

async fn get_card_diagram_presence(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<Vec<DiagramPresenceEntry>> {
    ensure_card_public_read(database(&state)?, card_id, Some(current.id)).await?;
    card_diagram_presence_snapshot(&state, card_id, current.id).await
}

async fn update_card_diagram_presence(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateDiagramPresenceRequest>) -> ApiResult<Vec<DiagramPresenceEntry>> {
    ensure_card_public_read(database(&state)?, card_id, Some(current.id)).await?;
    let account = diagram_live_account(&state, current).await?;
    let event = record_diagram_cursor(&state, card_id, current, &account, request.x, request.y).await?;
    let _ = state.diagram_events.send(event);
    card_diagram_presence_snapshot(&state, card_id, current.id).await
}

async fn load_card_diagram_notes(pool: &PgPool, card_id: Uuid) -> Result<Vec<DiagramNoteResponse>, ApiError> {
    let rows = sqlx::query_as::<_, DiagramNoteRow>(
        "SELECT n.id, n.x, n.y, n.created_by AS author_id, COALESCE(u.username, 'Deleted user') AS author_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS author_avatar_url, n.created_at::text AS created_at FROM card_diagram_notes n LEFT JOIN users u ON u.id = n.created_by WHERE n.card_id = $1 ORDER BY n.created_at ASC, n.id ASC",
    ).bind(card_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    if rows.is_empty() { return Ok(Vec::new()); }
    let note_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let comments = sqlx::query_as::<_, DiagramNoteCommentRow>(
        "SELECT c.id, c.note_id, c.author_id, COALESCE(u.username, 'Deleted user') AS author_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS author_avatar_url, c.body, c.created_at::text AS created_at FROM card_diagram_note_comments c LEFT JOIN users u ON u.id = c.author_id WHERE c.note_id = ANY($1) ORDER BY c.created_at ASC, c.id ASC",
    ).bind(&note_ids).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut comments_by_note = HashMap::<Uuid, Vec<DiagramNoteCommentResponse>>::new();
    for comment in comments {
        comments_by_note.entry(comment.note_id).or_default().push(DiagramNoteCommentResponse { id: comment.id, author_id: comment.author_id, author_name: comment.author_name, author_avatar_url: comment.author_avatar_url, body: comment.body, created_at: comment.created_at });
    }
    Ok(rows.into_iter().map(|row| DiagramNoteResponse { id: row.id, x: row.x, y: row.y, author_id: row.author_id, author_name: row.author_name, author_avatar_url: row.author_avatar_url, created_at: row.created_at, comments: comments_by_note.remove(&row.id).unwrap_or_default() }).collect())
}

async fn list_card_diagram_notes(State(state): State<AppState>, current: Viewer, Path(card_id): Path<Uuid>) -> ApiResult<Vec<DiagramNoteResponse>> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    Ok(Json(load_card_diagram_notes(pool, card_id).await?))
}

async fn create_card_diagram_note(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateDiagramNoteRequest>) -> ApiResult<DiagramNoteResponse> {
    if !(0..=20_000).contains(&request.x) || !(0..=20_000).contains(&request.y) { return Err(ApiError::bad_request("Diagram note coordinates must be between 0 and 20000.")); }
    let body = valid_text(&request.body, "body", 4_000)?;
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let note_id = Uuid::new_v4();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO card_diagram_notes (id, card_id, x, y, created_by) VALUES ($1, $2, $3, $4, $5)")
        .bind(note_id).bind(card_id).bind(request.x).bind(request.y).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO card_diagram_note_comments (id, note_id, author_id, body) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4()).bind(note_id).bind(current.id).bind(body).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    let note = load_card_diagram_notes(pool, card_id).await?.into_iter().find(|note| note.id == note_id)
        .ok_or_else(|| ApiError::internal(sqlx::Error::RowNotFound))?;
    let _ = state.diagram_events.send(DiagramLiveEvent::NotesChanged(DiagramNotesChangedEvent { event_type: "diagram_notes_changed", card_id }));
    let _ = state.events.send(());
    Ok(Json(note))
}

async fn create_card_diagram_note_comment(State(state): State<AppState>, current: CurrentUser, Path(note_id): Path<Uuid>, Json(request): Json<CreateDiagramNoteCommentRequest>) -> ApiResult<DiagramNoteResponse> {
    let body = valid_text(&request.body, "body", 4_000)?;
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM card_diagram_notes WHERE id = $1")
        .bind(note_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "diagram_note_not_found", "Diagram note was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    sqlx::query("INSERT INTO card_diagram_note_comments (id, note_id, author_id, body) VALUES ($1, $2, $3, $4)")
        .bind(Uuid::new_v4()).bind(note_id).bind(current.id).bind(body).execute(pool).await.map_err(ApiError::internal)?;
    sqlx::query("UPDATE card_diagram_notes SET updated_at = now() WHERE id = $1").bind(note_id).execute(pool).await.map_err(ApiError::internal)?;
    let note = load_card_diagram_notes(pool, card_id).await?.into_iter().find(|note| note.id == note_id)
        .ok_or_else(|| ApiError::internal(sqlx::Error::RowNotFound))?;
    let _ = state.diagram_events.send(DiagramLiveEvent::NotesChanged(DiagramNotesChangedEvent { event_type: "diagram_notes_changed", card_id }));
    let _ = state.events.send(());
    Ok(Json(note))
}

async fn delete_card_diagram_note(State(state): State<AppState>, current: CurrentUser, Path(note_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let (card_id, author_id) = sqlx::query_as::<_, (Uuid, Option<Uuid>)>("SELECT card_id, created_by FROM card_diagram_notes WHERE id = $1")
        .bind(note_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "diagram_note_not_found", "Diagram note was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if author_id != Some(current.id) {
        ensure_card_full_access(pool, card_id, current.id).await?;
    }
    let deleted = sqlx::query("DELETE FROM card_diagram_notes WHERE id = $1")
        .bind(note_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "diagram_note_not_found", "Diagram note was not found.".to_owned()));
    }
    record_card_activity(pool, card_id, current.id, "Удалена заметка на схеме", "").await;
    let _ = state.diagram_events.send(DiagramLiveEvent::NotesChanged(DiagramNotesChangedEvent { event_type: "diagram_notes_changed", card_id }));
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn get_card_diagram(State(state): State<AppState>, current: Viewer, Path(card_id): Path<Uuid>) -> ApiResult<Option<DiagramResponse>> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    let diagram = sqlx::query_as::<_, DiagramResponse>("SELECT id, card_id, title, document, version FROM card_diagrams WHERE card_id = $1")
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    Ok(Json(diagram))
}

async fn replace_card_diagram(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceDiagramRequest>) -> ApiResult<DiagramResponse> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let title = valid_text(&request.title, "title", 120)?;
    validate_diagram_document(&request.document)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let previous = sqlx::query_as::<_, DiagramResponse>("SELECT id, card_id, title, document, version FROM card_diagrams WHERE card_id = $1 FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?;
    if let Some(previous) = &previous {
        if request.version != Some(previous.version) {
            return Err(ApiError(StatusCode::CONFLICT, "diagram_conflict", "The diagram was updated elsewhere. Reload it before saving.".to_owned()));
        }
    } else if request.version.is_some() {
        return Err(ApiError(StatusCode::CONFLICT, "diagram_conflict", "The diagram no longer exists. Reload it before saving.".to_owned()));
    }
    let diagram = if let Some(previous) = previous {
        sqlx::query_as::<_, DiagramResponse>("UPDATE card_diagrams SET title = $1, document = $2, version = version + 1, updated_at = now() WHERE id = $3 RETURNING id, card_id, title, document, version")
            .bind(title).bind(request.document).bind(previous.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    } else {
        sqlx::query_as::<_, DiagramResponse>("INSERT INTO card_diagrams (id, card_id, title, document, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING id, card_id, title, document, version")
            .bind(Uuid::new_v4()).bind(card_id).bind(title).bind(request.document).bind(current.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    };
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card_id, current.id, "Обновлена схема", &diagram.title).await;
    let _ = state.events.send(());
    Ok(Json(diagram))
}

async fn apply_card_diagram_merge(state: &AppState, current: CurrentUser, card_id: Uuid, request: DiagramMergeRequest) -> Result<(DiagramResponse, Option<DiagramMergeEvent>), ApiError> {
    let pool = database(state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    validate_diagram_document(&request.base_document)?;
    validate_diagram_document(&request.document)?;
    ensure_diagram_object_locks(state, card_id, current, &request.base_document, &request.document).await?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let previous = sqlx::query_as::<_, DiagramResponse>("SELECT id, card_id, title, document, version FROM card_diagrams WHERE card_id = $1 FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?;
    let already_applied = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM card_diagram_operations WHERE id = $1)")
        .bind(request.operation_id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    if already_applied {
        let diagram = previous.ok_or_else(|| ApiError::bad_request("The diagram operation has no document."))?;
        transaction.commit().await.map_err(ApiError::internal)?;
        return Ok((diagram, None));
    }
    let previous_title = previous.as_ref().map(|diagram| diagram.title.clone()).unwrap_or_else(|| "Схема".to_owned());
    let merged_title = if request.title == request.base_title { previous_title } else { valid_text(&request.title, "title", 120)?.to_owned() };
    let current_document = previous.as_ref().map(|diagram| diagram.document.clone()).unwrap_or_else(|| json!({ "strokes": [], "elements": [] }));
    let document = merge_diagram_documents(current_document, &request.base_document, &request.document)?;
    validate_diagram_document(&document)?;
    let diagram = if let Some(previous) = previous {
        sqlx::query_as::<_, DiagramResponse>("UPDATE card_diagrams SET title = $1, document = $2, version = version + 1, updated_at = now() WHERE id = $3 RETURNING id, card_id, title, document, version")
            .bind(&merged_title).bind(&document).bind(previous.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    } else {
        sqlx::query_as::<_, DiagramResponse>("INSERT INTO card_diagrams (id, card_id, title, document, created_by) VALUES ($1, $2, $3, $4, $5) RETURNING id, card_id, title, document, version")
            .bind(Uuid::new_v4()).bind(card_id).bind(&merged_title).bind(&document).bind(current.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?
    };
    sqlx::query("INSERT INTO card_diagram_operations (id, card_id, actor_id) VALUES ($1, $2, $3)")
        .bind(request.operation_id).bind(card_id).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    let event = DiagramMergeEvent { event_type: "diagram_merge", card_id, operation_id: request.operation_id, actor_id: current.id, title: diagram.title.clone(), base_title: request.base_title, base_document: request.base_document, document: request.document };
    Ok((diagram, Some(event)))
}

async fn sync_card_diagram(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<DiagramMergeRequest>) -> ApiResult<DiagramResponse> {
    let (diagram, event) = apply_card_diagram_merge(&state, current, card_id, request).await?;
    if let Some(event) = event { let _ = state.diagram_events.send(DiagramLiveEvent::Merge(event)); let _ = state.events.send(()); }
    Ok(Json(diagram))
}

async fn card_diagram_websocket(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, upgrade: WebSocketUpgrade) -> Result<Response, ApiError> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, Some(current.id)).await?;
    let can_edit = ensure_card_permission(pool, card_id, current.id, "edit_cards").await.is_ok();
    let account = diagram_live_account(&state, current).await?;
    Ok(upgrade.on_upgrade(move |socket| run_card_diagram_websocket(state, card_id, current, account, can_edit, socket)))
}

async fn run_card_diagram_websocket(state: AppState, card_id: Uuid, current: CurrentUser, account: FreeformLiveAccount, can_edit: bool, socket: WebSocket) {
    let (mut sender, mut receiver) = futures_util::StreamExt::split(socket);
    let mut events = state.diagram_events.subscribe();
    loop {
        tokio::select! {
            incoming = futures_util::StreamExt::next(&mut receiver) => {
                let Some(Ok(Message::Text(payload))) = incoming else { break; };
                let Ok(request) = serde_json::from_str::<DiagramSocketRequest>(&payload) else { continue; };
                match request {
                    DiagramSocketRequest::Merge(request) => match apply_card_diagram_merge(&state, current, card_id, request).await {
                        Ok((_, Some(event))) => { let _ = state.diagram_events.send(DiagramLiveEvent::Merge(event)); let _ = state.events.send(()); }
                        Ok((_, None)) => {}
                        // A stale or locked local operation must not take the
                        // whole real-time channel down: the client can keep
                        // receiving the collaborator's subsequent edits.
                        Err(_) => {}
                    },
                    DiagramSocketRequest::Live(DiagramLiveSocketRequest::Cursor { x, y }) => match record_diagram_cursor(&state, card_id, current, &account, x, y).await {
                        Ok(event) => { let _ = state.diagram_events.send(event); }
                        Err(_) => break,
                    },
                    DiagramSocketRequest::Live(DiagramLiveSocketRequest::ObjectLock { object_id, active }) => {
                        if !can_edit || object_id.is_empty() || object_id.len() > 160 { continue; }
                        let event = record_diagram_object_lock(&state, card_id, current, &account, object_id, active).await;
                        let _ = state.diagram_events.send(event);
                    }
                }
            }
            event = events.recv() => {
                let Ok(event) = event else { continue; };
                if diagram_live_event_card_id(&event) != card_id { continue; }
                let Ok(payload) = diagram_live_event_payload(&event) else { continue; };
                if sender.send(Message::Text(payload.into())).await.is_err() { break; }
            }
        }
    }
}

async fn create_checklist(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateChecklistRequest>) -> ApiResult<ChecklistResponse> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 200)?;
    let pool = database(&state)?;
    let checklist = sqlx::query_as::<_, ChecklistRow>(
        "INSERT INTO checklists (id, card_id, title, position) VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) FROM checklists WHERE card_id = $2), 0) + 1000) RETURNING id, title",
    )
    .bind(Uuid::new_v4())
    .bind(card_id)
    .bind(title)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    record_card_activity(pool, card_id, actor_id, "Создан чек-лист", &checklist.title).await;
    let _ = state.events.send(());
    Ok(Json(ChecklistResponse { id: checklist.id, title: checklist.title, items: vec![] }))
}

async fn reorder_checklists(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReorderChecklistsRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if request.checklist_ids.len() > 200 || request.checklist_ids.len() != request.checklist_ids.iter().collect::<HashSet<_>>().len() {
        return Err(ApiError::bad_request("Checklist order contains duplicate or invalid entries."));
    }
    let existing_ids = sqlx::query_scalar::<_, Uuid>("SELECT id FROM checklists WHERE card_id = $1 ORDER BY position, id")
        .bind(card_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let requested_ids = request.checklist_ids.iter().copied().collect::<HashSet<_>>();
    if existing_ids.len() != request.checklist_ids.len() || existing_ids.iter().collect::<HashSet<_>>() != requested_ids.iter().collect::<HashSet<_>>() {
        return Err(ApiError::bad_request("Checklist order must include every checklist of this card exactly once."));
    }
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    // `(card_id, position)` is unique. Swapping adjacent checklists directly
    // would temporarily reuse the neighbour's position and PostgreSQL would
    // reject the otherwise valid reorder. Reserve unique transient positions
    // first, then write the final stable order in the same transaction.
    for (index, checklist_id) in request.checklist_ids.iter().enumerate() {
        sqlx::query("UPDATE checklists SET position = $1 WHERE id = $2 AND card_id = $3")
            .bind(-1_000_000_000_i64 - index as i64).bind(checklist_id).bind(card_id)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    for (index, checklist_id) in request.checklist_ids.iter().enumerate() {
        sqlx::query("UPDATE checklists SET position = $1 WHERE id = $2 AND card_id = $3")
            .bind(((index + 1) * 1000) as i64).bind(checklist_id).bind(card_id)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card_id, current.id, "Изменён порядок чек-листов", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_checklist(State(state): State<AppState>, current: CurrentUser, Path(checklist_id): Path<Uuid>, Json(request): Json<UpdateChecklistRequest>) -> ApiResult<ChecklistRow> {
    let title = valid_text(&request.title, "title", 200)?;
    let pool = database(&state)?;
    let (card_id, previous_title) = sqlx::query_as::<_, (Uuid, String)>("SELECT card_id, title FROM checklists WHERE id = $1")
        .bind(checklist_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let checklist = sqlx::query_as::<_, ChecklistRow>(
        "UPDATE checklists cl SET title = $1 FROM cards c WHERE cl.id = $2 AND cl.card_id = c.id AND c.archived_at IS NULL RETURNING cl.id, cl.title",
    )
    .bind(&title).bind(checklist_id).fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    if previous_title != title {
        record_card_activity(pool, card_id, current.id, "Переименован чек-лист", &format!("{previous_title} → {title}")).await;
        let _ = state.events.send(());
    }
    Ok(Json(checklist))
}

async fn delete_checklist(State(state): State<AppState>, current: CurrentUser, Path(checklist_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM checklists WHERE id = $1")
        .bind(checklist_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let attachment_keys = sqlx::query_scalar::<_, Option<String>>("SELECT a.object_key FROM attachments a INNER JOIN checklist_items i ON i.id = a.checklist_item_id WHERE i.checklist_id = $1")
        .bind(checklist_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let checklist = sqlx::query_as::<_, ChecklistActivityRow>("DELETE FROM checklists cl USING cards c, boards b, board_members bm WHERE cl.id = $1 AND cl.card_id = c.id AND c.board_id = b.id AND bm.board_id = b.id AND bm.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) RETURNING cl.card_id, cl.title")
        .bind(checklist_id).bind(current.id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    record_card_activity(pool, checklist.card_id, current.id, "Удалён чек-лист", &checklist.title).await;
    for key in attachment_keys.into_iter().flatten() { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn create_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(checklist_id): Path<Uuid>, Json(request): Json<CreateChecklistItemRequest>) -> ApiResult<ChecklistItemResponse> {
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 500)?;
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM checklists WHERE id = $1")
        .bind(checklist_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, actor_id, "edit_cards").await?;
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "INSERT INTO checklist_items (id, checklist_id, card_id, title, position) SELECT $1, cl.id, cl.card_id, $2, COALESCE((SELECT MAX(position) FROM checklist_items WHERE checklist_id = cl.id), 0) + 1000 FROM checklists cl JOIN cards c ON c.id = cl.card_id JOIN boards b ON b.id = c.board_id JOIN board_members bm ON bm.board_id = b.id WHERE cl.id = $3 AND c.archived_at IS NULL AND bm.user_id = $4 AND flowboard_has_permission(b.workspace_id, $4, 'edit_cards'::workspace_permission) RETURNING id, card_id, title, is_completed, description, (SELECT title FROM checklists WHERE id = checklist_id) AS checklist_title",
    )
    .bind(Uuid::new_v4()).bind(title).bind(checklist_id).bind(actor_id)
    .fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    record_card_activity(pool, item.card_id, actor_id, "Добавлен пункт в чек-лист", &format!("{}: {}", item.checklist_title, item.title)).await;
    let _ = state.events.send(());
    Ok(Json(ChecklistItemResponse { id: item.id, title: item.title, is_completed: item.is_completed, description: item.description, attachments: vec![] }))
}

async fn update_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(item_id): Path<Uuid>, Json(request): Json<UpdateChecklistItemRequest>) -> ApiResult<ChecklistItemResponse> {
    let actor_id = current.id;
    let pool = database(&state)?;
    let (card_id, previous_title) = sqlx::query_as::<_, (Uuid, String)>("SELECT card_id, title FROM checklist_items WHERE id = $1")
        .bind(item_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, actor_id, "edit_cards").await?;
    let title = request.title.as_deref().map(|value| valid_text(value, "title", 500).map(ToOwned::to_owned)).transpose()?;
    let description = request.description.as_deref().map(|value| {
        if value.chars().count() > 4_000 { Err(ApiError::bad_request("Checklist item description must be at most 4000 characters.")) }
        else { Ok(value.to_owned()) }
    }).transpose()?;
    if title.is_none() && request.is_completed.is_none() && description.is_none() { return Err(ApiError::bad_request("Provide a checklist item change.")); }
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "UPDATE checklist_items i SET title = COALESCE($1, i.title), is_completed = COALESCE($2, i.is_completed), completed_at = CASE WHEN COALESCE($2, i.is_completed) THEN COALESCE(i.completed_at, now()) ELSE NULL END, completed_by = CASE WHEN COALESCE($2, i.is_completed) THEN COALESCE(i.completed_by, $3) ELSE NULL END, description = COALESCE($4, i.description) FROM cards c INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE i.id = $5 AND i.card_id = c.id AND c.archived_at IS NULL AND m.user_id = $3 AND flowboard_has_permission(b.workspace_id, $3, 'edit_cards'::workspace_permission) RETURNING i.id, i.card_id, i.title, i.is_completed, i.description, (SELECT title FROM checklists WHERE id = i.checklist_id) AS checklist_title",
    )
    .bind(title.as_deref())
    .bind(request.is_completed)
    .bind(actor_id)
    .bind(description)
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    let action = if title.is_some() { "Переименован пункт чек-листа" } else if request.description.is_some() { "Изменено описание пункта чек-листа" } else if item.is_completed { "Отмечен пункт чек-листа" } else { "Снята отметка с пункта" };
    if request.description.is_some() { replace_card_mentions(pool, item.card_id, actor_id, "checklist_item_description", item.id, &item.description).await?; }
    let detail = if title.is_some() { format!("{}: {} → {}", item.checklist_title, previous_title, item.title) } else { format!("{}: {}", item.checklist_title, item.title) };
    record_card_activity(pool, item.card_id, actor_id, action, &detail).await;
    let _ = state.events.send(());
    let attachments = sqlx::query_as::<_, AttachmentResponse>("SELECT id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS url FROM attachments WHERE checklist_item_id = $1 ORDER BY created_at DESC")
        .bind(item.id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(ChecklistItemResponse { id: item.id, title: item.title, is_completed: item.is_completed, description: item.description, attachments }))
}

async fn delete_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(item_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let actor_id = current.id;
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM checklist_items WHERE id = $1")
        .bind(item_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, actor_id, "edit_cards").await?;
    let attachment_keys = sqlx::query_scalar::<_, Option<String>>("SELECT object_key FROM attachments WHERE checklist_item_id = $1")
        .bind(item_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "DELETE FROM checklist_items i USING cards c, boards b, board_members m WHERE i.id = $1 AND i.card_id = c.id AND c.board_id = b.id AND m.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) RETURNING i.id, i.card_id, i.title, i.is_completed, i.description, (SELECT title FROM checklists WHERE id = i.checklist_id) AS checklist_title",
    )
    .bind(item_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    record_card_activity(pool, item.card_id, actor_id, "Удалён пункт из чек-листа", &format!("{}: {}", item.checklist_title, item.title)).await;
    for key in attachment_keys.into_iter().flatten() { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn create_comment(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateCommentRequest>) -> ApiResult<CommentResponse> {
    let actor_id = current.id;
    let body = valid_text(&request.body, "body", 10_000)?;
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, actor_id, "edit_cards").await?;
    if let Some(parent_id) = request.parent_comment_id {
        let parent_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM comments WHERE id = $1 AND card_id = $2 AND parent_comment_id IS NULL)")
            .bind(parent_id).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !parent_exists { return Err(ApiError::bad_request("A thread can only be started from a main card comment.")); }
    }
    let comment_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO comments (id, card_id, author_id, body, parent_comment_id) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(card_id)
    .bind(actor_id)
    .bind(&body)
    .bind(request.parent_comment_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    replace_card_mentions(pool, card_id, actor_id, "comment", comment_id, &body).await?;
    let comment = load_card_comments(pool, card_id, Some(actor_id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    record_card_activity(pool, card_id, actor_id, if comment.parent_comment_id.is_some() { "Добавлено сообщение в тред" } else { "Добавлен комментарий" }, "").await;
    // A Flowboard discussion thread is private to the card UI. Only a fresh,
    // local root comment is mirrored to the Discord thread; Discord imports
    // enter through `create_discord_comment` and never reach this path.
    if comment.parent_comment_id.is_none() {
        push_local_comment_to_discord(&state, card_id, &comment);
    }
    let _ = state.events.send(());
    Ok(Json(comment))
}

fn push_local_comment_to_discord(state: &AppState, card_id: Uuid, comment: &CommentResponse) {
    let Some(push) = state.comment_push.clone() else { return; };
    let attachments = comment.attachments.iter().map(|attachment| json!({
        "id": attachment.id.to_string(),
        "original_name": attachment.original_name,
        "media_type": attachment.media_type,
        "byte_size": attachment.byte_size,
        "download_url": attachment.download_url,
    })).collect::<Vec<_>>();
    let author_avatar_url = comment.author_avatar_url.as_deref()
        .and_then(|url| absolute_flowboard_url(push.public_base_url.as_ref(), url));
    let payload = json!({
        "card_id": card_id.to_string(),
        "comment": {
            "id": comment.id.to_string(),
            "body": discord_outbound_comment_body(&comment.body),
            "author_name": comment.author_name,
            "author_avatar_url": author_avatar_url,
            "attachments": attachments,
        },
    });
    let client = state.external_http.clone();
    tokio::spawn(async move {
        // The receiver is idempotent by comment.id. Retry only transient
        // transport/server failures; bad credentials and malformed requests
        // must be surfaced in logs instead of being retried forever.
        for attempt in 0..3 {
            match client.post(push.endpoint.clone())
                .bearer_auth(&push.token)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(payload.to_string())
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return,
                Ok(response) if response.status().is_client_error() => {
                    tracing::error!(status = %response.status(), card_id = %card_id, "Discord comment push was rejected");
                    return;
                }
                Ok(response) => {
                    tracing::warn!(status = %response.status(), card_id = %card_id, attempt, "Discord comment push failed; retrying");
                }
                Err(error) => {
                    tracing::warn!(?error, card_id = %card_id, attempt, "Discord comment push request failed; retrying");
                }
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_secs(5 * (attempt + 1))).await;
            }
        }
        tracing::error!(card_id = %card_id, "Discord comment push failed after retries");
    });
}

fn absolute_flowboard_url(base_url: Option<&reqwest::Url>, value: &str) -> Option<String> {
    if let Ok(url) = reqwest::Url::parse(value) {
        return matches!(url.scheme(), "http" | "https").then(|| url.into());
    }
    base_url.and_then(|base_url| base_url.join(value.trim_start_matches('/')).ok())
        .map(|url| url.into())
}

async fn get_comment_thread(State(state): State<AppState>, current: Viewer, Path(comment_id): Path<Uuid>) -> ApiResult<CommentThreadResponse> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
        .bind(comment_id)
        .fetch_optional(pool)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "comment_not_found", "Comment was not found.".to_owned()))?;
    let actor_id = current.0.map(|user| user.id);
    ensure_card_public_read(pool, card_id, actor_id).await?;
    let mut comments = load_card_comments(pool, card_id, actor_id).await?;
    let root_index = comments.iter().position(|comment| comment.id == comment_id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "comment_not_found", "Comment was not found.".to_owned()))?;
    let mut root = comments.remove(root_index);
    let mut thread_comments: Vec<CommentResponse> = comments.into_iter()
        .filter(|comment| comment.parent_comment_id == Some(comment_id))
        .collect();
    thread_comments.sort_by(|left, right| left.created_at.cmp(&right.created_at).then(left.id.cmp(&right.id)));
    if actor_id.is_none() {
        let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM cards WHERE id = $1")
            .bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        let rewrite_avatar = |comment: &mut CommentResponse| {
            if let Some(author_id) = comment.author_id {
                comment.author_avatar_url = Some(format!("/v1/public/boards/{board_id}/avatars/{author_id}"));
            } else if comment.author_avatar_url.is_some() {
                comment.author_avatar_url = Some(format!("/v1/comments/{}/avatar", comment.id));
            }
        };
        rewrite_avatar(&mut root);
        for comment in &mut thread_comments { rewrite_avatar(comment); }
    } else {
        if root.author_id.is_none() && root.author_avatar_url.is_some() { root.author_avatar_url = Some(format!("/v1/comments/{}/avatar", root.id)); }
        for comment in &mut thread_comments {
            if comment.author_id.is_none() && comment.author_avatar_url.is_some() { comment.author_avatar_url = Some(format!("/v1/comments/{}/avatar", comment.id)); }
        }
    }
    Ok(Json(CommentThreadResponse { root, comments: thread_comments }))
}

async fn create_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Json(request): Json<CreateDiscordCardRequest>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    let source_id = valid_text(&request.source_id, "source_id", 128)?.to_owned();
    if let Some(card) = sqlx::query_as::<_, CardResponse>("SELECT id, list_id, title, description, start_at::text AS start_at FROM cards WHERE discord_integration_id = $1 AND discord_source_id = $2")
        .bind(integration.id).bind(&source_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
        return Ok(Json(card));
    }
    let title = valid_text(&request.title, "title", 500)?;
    let description = request.description.trim();
    if description.chars().count() > 20_000 { return Err(ApiError::bad_request("description must not exceed 20000 characters.")); }
    let target_list_id = request.list_id.or(integration.default_list_id)
        .ok_or_else(|| ApiError::bad_request("list_id is required because this token has no default list."))?;
    let card = sqlx::query_as::<_, CardResponse>(
        "INSERT INTO cards (id, board_id, list_id, title, description, position, created_by, discord_integration_id, discord_source_id) SELECT $1, l.board_id, l.id, $2, $3, COALESCE((SELECT MAX(position) FROM cards WHERE list_id = l.id), 0) + 1000, NULL, $4, $5 FROM lists l INNER JOIN boards b ON b.id = l.board_id WHERE l.id = $6 AND l.board_id = $7 AND b.archived_at IS NULL RETURNING id, list_id, title, description, start_at::text AS start_at",
    )
    .bind(Uuid::new_v4()).bind(title).bind(description).bind(integration.id).bind(&source_id).bind(target_list_id).bind(integration.board_id)
    .fetch_optional(pool).await.map_err(ApiError::internal)?;
    let card = match card {
        Some(card) => card,
        None => sqlx::query_as::<_, CardResponse>("SELECT id, list_id, title, description, start_at::text AS start_at FROM cards WHERE discord_integration_id = $1 AND discord_source_id = $2")
            .bind(integration.id).bind(&source_id).fetch_optional(pool).await.map_err(ApiError::internal)?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "discord_target_not_found", "Discord target list is no longer available.".to_owned()))?,
    };
    record_external_card_activity(pool, card.id, "Discord: создана задача", &card.title).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn list_discord_board_lists(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<ListResponse>> {
    let lists = sqlx::query_as::<_, ListResponse>("SELECT id, title, grid_column, grid_row, card_limit, is_public FROM lists WHERE board_id = $1 ORDER BY position, id")
        .bind(integration.board_id)
        .fetch_all(database(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(lists))
}

async fn list_discord_profile_roles(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<ProfileRoleResponse>> {
    // Roles are system-wide definitions, but a Discord token must only see
    // roles that are actually assigned to at least one member of its board.
    // This also gives the bridge an exact UUID for an incoming Discord role
    // mention without exposing an unrelated workspace's role catalogue.
    let roles = sqlx::query_as::<_, ProfileRoleResponse>(
        "SELECT pr.id, pr.name, pr.color, pr.icon_shape, pr.icon_color FROM profile_roles pr \
         WHERE EXISTS (SELECT 1 FROM user_profile_roles upr INNER JOIN board_members bm ON bm.user_id = upr.user_id WHERE upr.role_id = pr.id AND bm.board_id = $1) \
         ORDER BY lower(pr.name), pr.id",
    )
    .bind(integration.board_id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(roles))
}

async fn resolve_discord_board_users(State(state): State<AppState>, integration: DiscordIntegration, Json(request): Json<ResolveDiscordUsersRequest>) -> ApiResult<Vec<DiscordUserResolutionResponse>> {
    if request.discord_user_ids.len() > 100 {
        return Err(ApiError::bad_request("Resolve at most 100 Discord users per request."));
    }
    let discord_user_ids = request.discord_user_ids.iter()
        .map(|value| valid_discord_user_id(value))
        .collect::<Result<HashSet<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    if discord_user_ids.is_empty() { return Ok(Json(Vec::new())); }
    let users = sqlx::query_as::<_, DiscordUserResolutionResponse>(
        "SELECT dua.discord_user_id, u.id AS user_id, u.username FROM user_discord_accounts dua \
         INNER JOIN users u ON u.id = dua.user_id AND u.disabled_at IS NULL \
         INNER JOIN board_members bm ON bm.user_id = u.id \
         WHERE bm.board_id = $1 AND dua.discord_user_id = ANY($2) ORDER BY u.username, u.id",
    )
    .bind(integration.board_id)
    .bind(&discord_user_ids)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(users))
}

async fn list_discord_labels(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<LabelResponse>> {
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT id, name, color, icon_shape, icon_color FROM labels WHERE board_id = $1 ORDER BY name")
        .bind(integration.board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    Ok(Json(labels))
}

async fn create_discord_label(State(state): State<AppState>, integration: DiscordIntegration, Json(request): Json<CreateLabelRequest>) -> ApiResult<LabelResponse> {
    let name = valid_text(&request.name, "name", 60)?;
    let color = valid_label_color(&request.color)?;
    let icon_shape = valid_profile_role_shape(&request.icon_shape)?;
    let icon_color = valid_label_color(&request.icon_color)?;
    let label = sqlx::query_as::<_, LabelResponse>(
        "INSERT INTO labels (id, workspace_id, board_id, name, color, icon_shape, icon_color) SELECT $1, b.workspace_id, b.id, $2, $3, $4, $5 FROM boards b WHERE b.id = $6 AND b.archived_at IS NULL ON CONFLICT (board_id, name) DO UPDATE SET color = EXCLUDED.color, icon_shape = EXCLUDED.icon_shape, icon_color = EXCLUDED.icon_color RETURNING id, name, color, icon_shape, icon_color",
    )
    .bind(Uuid::new_v4()).bind(name).bind(color).bind(icon_shape).bind(icon_color).bind(integration.board_id)
    .fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Discord integration board was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(label))
}

async fn update_discord_label(State(state): State<AppState>, integration: DiscordIntegration, Path(label_id): Path<Uuid>, Json(request): Json<UpdateDiscordLabelRequest>) -> ApiResult<LabelResponse> {
    let pool = database(&state)?;
    let current = sqlx::query_as::<_, LabelResponse>("SELECT id, name, color, icon_shape, icon_color FROM labels WHERE id = $1 AND board_id = $2")
        .bind(label_id).bind(integration.board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "label_not_found", "Label was not found on this Discord integration board.".to_owned()))?;
    let name = match request.name { Some(value) => valid_text(&value, "name", 60)?.to_owned(), None => current.name };
    let color = match request.color { Some(value) => valid_label_color(&value)?, None => current.color };
    let icon_shape = match request.icon_shape { Some(value) => valid_profile_role_shape(&value)?.to_owned(), None => current.icon_shape };
    let icon_color = match request.icon_color { Some(value) => valid_label_color(&value)?, None => current.icon_color };
    let duplicate_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM labels WHERE board_id = $1 AND name = $2 AND id <> $3)")
        .bind(integration.board_id).bind(&name).bind(label_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if duplicate_exists { return Err(ApiError::bad_request("A label with this name already exists on the board.")); }
    let label = sqlx::query_as::<_, LabelResponse>("UPDATE labels SET name = $1, color = $2, icon_shape = $3, icon_color = $4 WHERE id = $5 AND board_id = $6 RETURNING id, name, color, icon_shape, icon_color")
        .bind(name).bind(color).bind(icon_shape).bind(icon_color).bind(label_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(label))
}

async fn delete_discord_label(State(state): State<AppState>, integration: DiscordIntegration, Path(label_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let deleted = sqlx::query("DELETE FROM labels WHERE id = $1 AND board_id = $2")
        .bind(label_id).bind(integration.board_id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "label_not_found", "Label was not found on this Discord integration board.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn replace_discord_card_labels(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceDiscordCardLabelsRequest>) -> ApiResult<Vec<LabelResponse>> {
    if request.label_ids.len() > 20 { return Err(ApiError::bad_request("A card can have at most 20 labels.")); }
    let label_ids: Vec<Uuid> = request.label_ids.into_iter().collect::<HashSet<_>>().into_iter().collect();
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let card_board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM cards WHERE id = $1 AND archived_at IS NULL FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?;
    if card_board_id != Some(integration.board_id) { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    let matching_labels: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM labels WHERE board_id = $1 AND id = ANY($2)")
        .bind(integration.board_id).bind(&label_ids).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    if matching_labels != label_ids.len() as i64 { return Err(ApiError::bad_request("Every label must belong to this Discord integration board.")); }
    let current_label_ids: HashSet<Uuid> = sqlx::query_scalar("SELECT label_id FROM card_labels WHERE card_id = $1")
        .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?.into_iter().collect();
    let requested_label_ids: HashSet<Uuid> = label_ids.iter().copied().collect();
    if current_label_ids == requested_label_ids {
        let labels = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = $1 ORDER BY l.name")
            .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
        transaction.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(labels));
    }
    sqlx::query("DELETE FROM card_labels WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    if !label_ids.is_empty() {
        sqlx::query("INSERT INTO card_labels (card_id, label_id) SELECT $1, label_id FROM UNNEST($2::uuid[]) AS selected_labels(label_id) ON CONFLICT DO NOTHING")
            .bind(card_id).bind(&label_ids).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = $1 ORDER BY l.name")
        .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_external_card_activity(pool, card_id, "Discord: обновлены метки", "").await;
    let _ = state.events.send(());
    Ok(Json(labels))
}

async fn add_discord_card_label(State(state): State<AppState>, integration: DiscordIntegration, Path((card_id, label_id)): Path<(Uuid, Uuid)>) -> ApiResult<Vec<LabelResponse>> {
    let current = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id INNER JOIN cards c ON c.id = cl.card_id WHERE cl.card_id = $1 AND c.board_id = $2 ORDER BY l.name")
        .bind(card_id).bind(integration.board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    let mut label_ids: Vec<Uuid> = current.iter().map(|label| label.id).collect();
    if !label_ids.contains(&label_id) { label_ids.push(label_id); }
    replace_discord_card_labels(State(state), integration, Path(card_id), Json(ReplaceDiscordCardLabelsRequest { label_ids })).await
}

async fn remove_discord_card_label(State(state): State<AppState>, integration: DiscordIntegration, Path((card_id, label_id)): Path<(Uuid, Uuid)>) -> ApiResult<Vec<LabelResponse>> {
    let current = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id INNER JOIN cards c ON c.id = cl.card_id WHERE cl.card_id = $1 AND c.board_id = $2 ORDER BY l.name")
        .bind(card_id).bind(integration.board_id).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    let label_ids: Vec<Uuid> = current.into_iter().filter_map(|label| (label.id != label_id).then_some(label.id)).collect();
    replace_discord_card_labels(State(state), integration, Path(card_id), Json(ReplaceDiscordCardLabelsRequest { label_ids })).await
}

async fn list_discord_board_cards(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<DiscordCardListResponse>> {
    let cards = sqlx::query_as::<_, DiscordCardListResponse>("SELECT id, list_id, title, description, priority, completed_at IS NOT NULL AS is_completed, completed_at::text AS completed_at FROM cards WHERE board_id = $1 AND archived_at IS NULL ORDER BY list_id, position")
        .bind(integration.board_id)
        .fetch_all(database(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(cards))
}

async fn load_discord_thread_card(pool: &PgPool, integration: DiscordIntegration, card_id: Uuid) -> Result<DiscordThreadCardResponse, ApiError> {
    sqlx::query_as::<_, DiscordThreadCardResponse>(
        "SELECT c.id, c.list_id, c.title, c.description, c.archived_at IS NOT NULL AS is_archived, c.archived_at::text AS archived_at, c.completed_at IS NOT NULL AS is_completed, c.completed_at::text AS completed_at, dct.thread_id FROM cards c LEFT JOIN discord_card_threads dct ON dct.card_id = c.id AND dct.integration_id = $1 WHERE c.id = $2 AND c.board_id = $3",
    )
    .bind(integration.id)
    .bind(card_id)
    .bind(integration.board_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned()))
}

async fn record_discord_card_thread_event(pool: &PgPool, card_id: Uuid, event_kind: &str) {
    if let Err(error) = sqlx::query("INSERT INTO discord_card_thread_events (integration_id, card_id, event_kind) SELECT integration_id, $1, $2 FROM discord_card_threads WHERE card_id = $1")
        .bind(card_id)
        .bind(event_kind)
        .execute(pool)
        .await
    {
        tracing::error!(?error, card_id = %card_id, event_kind, "discord thread sync event insert failed");
    }
}

async fn bind_discord_card_thread(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<BindDiscordThreadRequest>) -> ApiResult<DiscordThreadCardResponse> {
    let thread_id = valid_text(&request.thread_id, "thread_id", 128)?.to_owned();
    let pool = database(&state)?;
    if let Some(existing_card_id) = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM discord_card_threads WHERE integration_id = $1 AND thread_id = $2")
        .bind(integration.id).bind(&thread_id).fetch_optional(pool).await.map_err(ApiError::internal)?
    {
        if existing_card_id != card_id { return Err(ApiError::bad_request("This Discord thread is already linked to another card.")); }
    }
    let current_thread = sqlx::query_scalar::<_, String>("SELECT thread_id FROM discord_card_threads WHERE integration_id = $1 AND card_id = $2")
        .bind(integration.id).bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    if current_thread.as_deref() == Some(thread_id.as_str()) {
        return Ok(Json(load_discord_thread_card(pool, integration, card_id).await?));
    }
    let saved = sqlx::query("INSERT INTO discord_card_threads (integration_id, card_id, thread_id) SELECT $1, c.id, $2 FROM cards c WHERE c.id = $3 AND c.board_id = $4 ON CONFLICT (integration_id, card_id) DO UPDATE SET thread_id = EXCLUDED.thread_id, updated_at = now()")
        .bind(integration.id).bind(&thread_id).bind(card_id).bind(integration.board_id).execute(pool).await.map_err(ApiError::internal)?;
    if saved.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    record_discord_card_thread_event(pool, card_id, "thread_linked").await;
    let _ = state.events.send(());
    Ok(Json(load_discord_thread_card(pool, integration, card_id).await?))
}

async fn get_discord_thread_card(State(state): State<AppState>, integration: DiscordIntegration, Path(thread_id): Path<String>) -> ApiResult<DiscordThreadCardResponse> {
    let thread_id = valid_text(&thread_id, "thread_id", 128)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM discord_card_threads WHERE integration_id = $1 AND thread_id = $2")
        .bind(integration.id).bind(thread_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "discord_thread_not_found", "Discord thread is not linked to this integration.".to_owned()))?;
    Ok(Json(load_discord_thread_card(database(&state)?, integration, card_id).await?))
}

async fn restore_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>) -> ApiResult<DiscordThreadCardResponse> {
    let pool = database(&state)?;
    let restored = sqlx::query("UPDATE cards SET archived_at = NULL, updated_at = now() WHERE id = $1 AND board_id = $2 AND archived_at IS NOT NULL")
        .bind(card_id).bind(integration.board_id).execute(pool).await.map_err(ApiError::internal)?;
    let card = load_discord_thread_card(pool, integration, card_id).await?;
    if restored.rows_affected() > 0 {
        record_external_card_activity(pool, card_id, "Discord: карточка восстановлена", "").await;
        let _ = state.events.send(());
    }
    Ok(Json(card))
}

async fn list_discord_card_sync_events(State(state): State<AppState>, integration: DiscordIntegration, Query(query): Query<DiscordCardSyncQuery>) -> ApiResult<Vec<DiscordCardSyncEventResponse>> {
    let after = query.after.unwrap_or(0).max(0);
    let limit = i64::from(query.limit.unwrap_or(100).clamp(1, 200));
    let events = sqlx::query_as::<_, DiscordCardSyncEventResponse>(
        "SELECT e.id AS event_id, e.event_kind, e.created_at::text AS created_at, c.id, c.list_id, c.title, c.description, c.archived_at IS NOT NULL AS is_archived, c.archived_at::text AS archived_at, c.completed_at IS NOT NULL AS is_completed, c.completed_at::text AS completed_at, dct.thread_id FROM discord_card_thread_events e INNER JOIN cards c ON c.id = e.card_id INNER JOIN discord_card_threads dct ON dct.integration_id = e.integration_id AND dct.card_id = e.card_id WHERE e.integration_id = $1 AND e.id > $2 ORDER BY e.id ASC LIMIT $3",
    )
    .bind(integration.id)
    .bind(after)
    .bind(limit)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(events))
}

async fn load_discord_card_status(pool: &PgPool, integration: DiscordIntegration, card_id: Uuid) -> Result<DiscordCardStatusResponse, ApiError> {
    sqlx::query_as::<_, DiscordCardStatusResponse>("SELECT id, list_id, title, description, priority, completed_at IS NOT NULL AS is_completed, completed_at::text AS completed_at FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL")
        .bind(card_id).bind(integration.board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned()))
}

async fn get_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>) -> ApiResult<DiscordCardStatusResponse> {
    let card = load_discord_card_status(database(&state)?, integration, card_id).await?;
    Ok(Json(card))
}

async fn set_discord_card_completion(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardCompletionRequest>) -> ApiResult<DiscordCardStatusResponse> {
    let pool = database(&state)?;
    let current = load_discord_card_status(pool, integration, card_id).await?;
    if current.is_completed == request.is_completed { return Ok(Json(current)); }
    let card_ids = active_duplicate_group_ids(pool, card_id).await?;
    if request.is_completed { for duplicate_id in &card_ids { ensure_card_has_no_active_blockers(pool, *duplicate_id).await?; } }
    let updated = sqlx::query("UPDATE cards SET completed_at = CASE WHEN $1 THEN now() ELSE NULL END, updated_at = now() WHERE id = ANY($2) AND board_id = $3 AND archived_at IS NULL")
        .bind(request.is_completed).bind(&card_ids).bind(integration.board_id).execute(pool).await.map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    for changed_card_id in card_ids {
        let detail = if changed_card_id == card_id { "" } else { "Синхронизировано со связью «Дубликат»" };
        record_external_card_activity(pool, changed_card_id, if request.is_completed { "Discord: задача выполнена" } else { "Discord: задача возвращена в работу" }, detail).await;
    }
    let card = load_discord_card_status(pool, integration, card_id).await?;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn set_discord_card_priority(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardPriorityRequest>) -> ApiResult<DiscordCardStatusResponse> {
    if !(0..=5).contains(&request.priority) { return Err(ApiError::bad_request("priority must be between 0 and 5.")); }
    let pool = database(&state)?;
    let current = load_discord_card_status(pool, integration, card_id).await?;
    if current.priority == request.priority { return Ok(Json(current)); }
    let card = sqlx::query_as::<_, DiscordCardStatusResponse>("UPDATE cards SET priority = $1, updated_at = now() WHERE id = $2 AND board_id = $3 AND archived_at IS NULL RETURNING id, list_id, title, description, priority, completed_at IS NOT NULL AS is_completed, completed_at::text AS completed_at")
        .bind(request.priority).bind(card_id).bind(integration.board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned()))?;
    let detail = if request.priority == 0 { "Приоритет снят".to_owned() } else { format!("Приоритет: {}/5", request.priority) };
    record_external_card_activity(pool, card_id, "Discord: изменён приоритет", &detail).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn move_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<MoveDiscordCardRequest>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let card = sqlx::query_as::<_, CardResponse>(
        "WITH source AS (SELECT id, board_id FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL FOR UPDATE), target AS (SELECT id, board_id FROM lists WHERE id = $3 FOR UPDATE), anchor AS (SELECT c.position FROM cards c, target, source WHERE c.id = $4 AND c.list_id = target.id AND c.id <> source.id FOR UPDATE), previous AS (SELECT c.position FROM cards c, target, source WHERE c.list_id = target.id AND c.id <> source.id AND c.position < (SELECT position FROM anchor) ORDER BY c.position DESC LIMIT 1) UPDATE cards c SET list_id = target.id, position = CASE WHEN $4 IS NULL THEN (SELECT COALESCE(MAX(position), 0) + 1000 FROM cards WHERE list_id = target.id AND id <> c.id) WHEN (SELECT position FROM previous) IS NULL THEN (SELECT position - 1000 FROM anchor) ELSE ((SELECT position FROM previous) + (SELECT position FROM anchor)) / 2 END, updated_at = now() FROM source, target WHERE c.id = source.id AND source.board_id = target.board_id AND ($4 IS NULL OR EXISTS (SELECT 1 FROM anchor)) RETURNING c.id, c.list_id, c.title, c.description, c.start_at::text AS start_at",
    )
    .bind(card_id)
    .bind(integration.board_id)
    .bind(request.list_id)
    .bind(request.before_card_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_or_list_not_found", "Card, target list, or insertion anchor was not found on this Discord integration board.".to_owned()))?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_external_card_activity(pool, card.id, "Discord: карточка перемещена", "").await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn list_discord_card_comments(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Query(query): Query<DiscordCommentQuery>) -> ApiResult<Vec<CommentResponse>> {
    let pool = database(&state)?;
    let card_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND board_id = $2)")
        .bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !card_exists { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    let Some(after) = query.after else {
        let mut comments = load_card_comments(pool, card_id, None).await?;
        // A local Flowboard thread is deliberately not mirrored back into Discord.
        comments.retain(|comment| comment.parent_comment_id.is_none());
        rewrite_discord_comment_avatar_urls(pool, integration, card_id, &mut comments).await?;
        remove_discord_outbound_voice_messages(&mut comments);
        rewrite_discord_comment_attachment_urls(integration, card_id, &mut comments);
        rewrite_discord_outbound_comment_bodies(&mut comments);
        return Ok(Json(comments));
    };
    let cursor_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM comments WHERE id = $1 AND card_id = $2 AND parent_comment_id IS NULL)")
        .bind(after).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !cursor_exists { return Err(ApiError::bad_request("The comment cursor does not belong to this card.")); }
    let limit = i64::from(query.limit.unwrap_or(100).clamp(1, 200));
    let rows = sqlx::query_as::<_, CommentRow>(
        "SELECT c.id, c.body, c.author_id, COALESCE(u.username, c.external_author_name, 'Deleted user') AS author_name, COALESCE(CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END, c.external_author_avatar_url) AS author_avatar_url, (SELECT pr.color FROM user_profile_roles upr INNER JOIN profile_roles pr ON pr.id = upr.role_id WHERE upr.user_id = c.author_id ORDER BY pr.name, pr.id LIMIT 1) AS author_role_color, c.parent_comment_id, c.created_at::text AS created_at, c.edited_at::text AS edited_at FROM comments c LEFT JOIN users u ON u.id = c.author_id CROSS JOIN (SELECT created_at, id FROM comments WHERE id = $2 AND card_id = $1) anchor WHERE c.card_id = $1 AND c.parent_comment_id IS NULL AND (c.created_at, c.id) > (anchor.created_at, anchor.id) ORDER BY c.created_at ASC, c.id ASC LIMIT $3",
    )
    .bind(card_id).bind(after).bind(limit)
    .fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut comments = comment_responses(pool, rows, None).await?;
    load_comment_attachments(pool, card_id, &mut comments).await?;
    rewrite_discord_comment_avatar_urls(pool, integration, card_id, &mut comments).await?;
    remove_discord_outbound_voice_messages(&mut comments);
    rewrite_discord_comment_attachment_urls(integration, card_id, &mut comments);
    rewrite_discord_outbound_comment_bodies(&mut comments);
    Ok(Json(comments))
}

async fn set_discord_card_cover(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<SetDiscordCardCoverRequest>) -> ApiResult<Value> {
    if request.mode != "full" && request.mode != "top" { return Err(ApiError::bad_request("Cover mode must be full or top.")); }
    if request.attachment_id.is_some() == request.attachment_url.as_deref().map(str::trim).filter(|url| !url.is_empty()).is_some() {
        return Err(ApiError::bad_request("Provide exactly one of attachment_id or attachment_url."));
    }
    let pool = database(&state)?;
    let attachment_id = if let Some(attachment_id) = request.attachment_id {
        let is_valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attachments a INNER JOIN cards c ON c.id = a.card_id WHERE a.id = $1 AND a.card_id = $2 AND c.board_id = $3 AND c.archived_at IS NULL AND (a.media_type LIKE 'image/%' OR a.media_type LIKE 'video/%'))")
            .bind(attachment_id).bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !is_valid { return Err(ApiError::bad_request("Attachment must be an image or video on this card in the token's board.")); }
        attachment_id
    } else {
        let attachment_url = request.attachment_url.as_deref().unwrap().trim();
        sqlx::query_scalar::<_, Uuid>("SELECT a.id FROM attachments a INNER JOIN cards c ON c.id = a.card_id WHERE a.card_id = $1 AND c.board_id = $2 AND c.archived_at IS NULL AND a.external_url = $3 AND (a.media_type LIKE 'image/%' OR a.media_type LIKE 'video/%') ORDER BY a.created_at DESC LIMIT 1")
            .bind(card_id).bind(integration.board_id).bind(attachment_url).fetch_optional(pool).await.map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::bad_request("Image or video attachment URL was not found on this card."))?
    };
    let updated = sqlx::query("UPDATE cards SET cover_attachment_id = $1, cover_mode = $2, updated_at = now() WHERE id = $3 AND board_id = $4 AND archived_at IS NULL")
        .bind(attachment_id).bind(&request.mode).bind(card_id).bind(integration.board_id).execute(pool).await.map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    record_external_card_activity(pool, card_id, "Discord: установлена обложка", if request.mode == "full" { "фон" } else { "сверху" }).await;
    let _ = state.events.send(());
    Ok(Json(json!({ "attachment_id": attachment_id, "mode": request.mode })))
}

async fn archive_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let card_ids = active_duplicate_group_ids(pool, card_id).await?;
    let archived = sqlx::query_scalar::<_, Uuid>("UPDATE cards SET archived_at = now(), updated_at = now() WHERE id = ANY($1) AND board_id = $2 AND archived_at IS NULL RETURNING id")
        .bind(&card_ids).bind(integration.board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    if !archived.is_empty() {
        for changed_card_id in archived {
            let detail = if changed_card_id == card_id { "" } else { "Синхронизировано со связью «Дубликат»" };
            record_external_card_activity(pool, changed_card_id, "Discord: предложка архивирована", detail).await;
        }
        let _ = state.events.send(());
        return Ok(StatusCode::NO_CONTENT);
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND board_id = $2)")
        .bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if exists { Ok(StatusCode::NO_CONTENT) }
    else { Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())) }
}

async fn create_discord_comment(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<CreateDiscordCommentRequest>) -> ApiResult<CommentResponse> {
    let pool = database(&state)?;
    let message_id = valid_text(&request.message_id, "message_id", 128)?.to_owned();
    let card_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL)")
        .bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !card_exists { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    ensure_card_unfrozen(pool, card_id).await?;
    if request.mentioned_role_ids.len() > 20 { return Err(ApiError::bad_request("A Discord comment can mention at most 20 Flowboard roles.")); }
    if request.mentioned_user_ids.len() > 50 { return Err(ApiError::bad_request("A Discord comment can mention at most 50 Flowboard users.")); }
    let mentioned_role_ids: Vec<Uuid> = request.mentioned_role_ids.iter().copied().collect::<HashSet<_>>().into_iter().collect();
    if !mentioned_role_ids.is_empty() {
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT pr.id) FROM profile_roles pr WHERE pr.id = ANY($1) \
             AND EXISTS (SELECT 1 FROM user_profile_roles upr INNER JOIN board_members bm ON bm.user_id = upr.user_id WHERE upr.role_id = pr.id AND bm.board_id = $2)",
        )
        .bind(&mentioned_role_ids)
        .bind(integration.board_id)
        .fetch_one(pool)
        .await
        .map_err(ApiError::internal)?;
        if found != mentioned_role_ids.len() as i64 {
            return Err(ApiError::bad_request("Every mentioned Discord role must be available on this board."));
        }
    }
    let mentioned_user_ids: Vec<Uuid> = request.mentioned_user_ids.iter().copied().collect::<HashSet<_>>().into_iter().collect();
    if !mentioned_user_ids.is_empty() {
        let found: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM board_members bm INNER JOIN users u ON u.id = bm.user_id AND u.disabled_at IS NULL WHERE bm.board_id = $1 AND bm.user_id = ANY($2)")
            .bind(integration.board_id)
            .bind(&mentioned_user_ids)
            .fetch_one(pool)
            .await
            .map_err(ApiError::internal)?;
        if found != mentioned_user_ids.len() as i64 {
            return Err(ApiError::bad_request("Every mentioned Discord user must be an active member of this board."));
        }
    }
    let author_name = valid_text(&request.author_name, "author_name", 120)?.to_owned();
    let author_avatar_url = request.author_avatar_url.as_deref().map(valid_discord_asset_url).transpose()?.map(ToOwned::to_owned);
    let mut attachment_rows = Vec::new();
    let mut parts = Vec::new();
    if !request.body.trim().is_empty() { parts.push(request.body.trim().to_owned()); }
    for attachment in &request.attachments {
        let url = valid_discord_asset_url(&attachment.url)?.to_owned();
        let filename = valid_text(&attachment.filename, "attachment filename", 255)?.to_owned();
        let discord_reference = match (&attachment.channel_id, &attachment.message_id, &attachment.attachment_id) {
            (None, None, None) => (None, None, None),
            (Some(channel_id), Some(message_id), Some(attachment_id)) => (
                Some(valid_text(channel_id, "Discord channel_id", 128)?.to_owned()),
                Some(valid_text(message_id, "Discord message_id", 128)?.to_owned()),
                Some(valid_text(attachment_id, "Discord attachment_id", 128)?.to_owned()),
            ),
            _ => return Err(ApiError::bad_request("Discord attachment metadata must include channel_id, message_id, and attachment_id together.")),
        };
        if !matches!(attachment.media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "video/mp4" | "video/webm" | "video/quicktime") {
            return Err(ApiError::bad_request("Discord attachments must be JPEG, PNG, GIF, WebP, MP4, WebM, or MOV."));
        }
        if !(0..=50 * 1024 * 1024).contains(&attachment.byte_size) { return Err(ApiError::bad_request("Discord attachment size must be between 0 and 50 MiB.")); }
        attachment_rows.push(DiscordAttachmentUpsert {
            id: Uuid::new_v4(), url, filename, media_type: attachment.media_type.clone(), byte_size: attachment.byte_size,
            channel_id: discord_reference.0, message_id: discord_reference.1, attachment_id: discord_reference.2,
        });
    }

    if let Some((comment_id, existing_card_id, existing_body)) = sqlx::query_as::<_, (Uuid, Uuid, String)>("SELECT id, card_id, body FROM comments WHERE discord_integration_id = $1 AND discord_message_id = $2")
        .bind(integration.id).bind(&message_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
        if existing_card_id != card_id { return Err(ApiError::bad_request("Discord comment belongs to another card.")); }

        // Replays are normal during media resync. Keep the Flowboard UUIDs
        // embedded in the comment body, but replace stale CDN URLs and add the
        // durable source identifiers required by the refresh endpoint.
        let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
        let existing_attachments = sqlx::query_as::<_, CommentAttachmentReference>(
            "SELECT id, discord_channel_id, discord_message_id, discord_attachment_id FROM attachments WHERE card_id = $1 AND $2 LIKE '%' || '/v1/attachments/' || id::text || '/content' || '%' ORDER BY strpos($2, '/v1/attachments/' || id::text || '/content')",
        )
        .bind(card_id).bind(&existing_body).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
        let mut used_attachment_ids = HashSet::new();
        let mut appended_media = Vec::new();
        for attachment in attachment_rows {
            let exact_match = attachment.attachment_id.as_deref().and_then(|discord_attachment_id| existing_attachments.iter().find(|existing| {
                !used_attachment_ids.contains(&existing.id)
                    && existing.discord_channel_id.as_deref() == attachment.channel_id.as_deref()
                    && existing.discord_message_id.as_deref() == attachment.message_id.as_deref()
                    && existing.discord_attachment_id.as_deref() == Some(discord_attachment_id)
            }));
            // Legacy rows have no stable source identifier. Discord preserves
            // attachment order within a message, so pair those rows in order.
            let existing = exact_match.or_else(|| existing_attachments.iter().find(|existing| {
                !used_attachment_ids.contains(&existing.id) && existing.discord_attachment_id.is_none()
            }));
            if let Some(existing) = existing {
                sqlx::query("UPDATE attachments SET external_url = $1, original_name = $2, media_type = $3, byte_size = $4, discord_channel_id = $5, discord_message_id = $6, discord_attachment_id = $7 WHERE id = $8")
                    .bind(&attachment.url).bind(&attachment.filename).bind(&attachment.media_type).bind(attachment.byte_size)
                    .bind(&attachment.channel_id).bind(&attachment.message_id).bind(&attachment.attachment_id).bind(existing.id)
                    .execute(&mut *transaction).await.map_err(ApiError::internal)?;
                used_attachment_ids.insert(existing.id);
            } else {
                sqlx::query("INSERT INTO attachments (id, card_id, uploaded_by, object_key, original_name, media_type, byte_size, external_url, discord_channel_id, discord_message_id, discord_attachment_id) VALUES ($1, $2, NULL, NULL, $3, $4, $5, $6, $7, $8, $9)")
                    .bind(attachment.id).bind(card_id).bind(&attachment.filename).bind(&attachment.media_type).bind(attachment.byte_size)
                    .bind(&attachment.url).bind(&attachment.channel_id).bind(&attachment.message_id).bind(&attachment.attachment_id)
                    .execute(&mut *transaction).await.map_err(ApiError::internal)?;
                appended_media.push(discord_media_markdown(&attachment.filename, &attachment.media_type, &format!("/v1/attachments/{}/content", attachment.id)));
            }
        }
        if !appended_media.is_empty() {
            let separator = if existing_body.is_empty() { "" } else { "\n" };
            let body = valid_text(&format!("{existing_body}{separator}{}", appended_media.join("\n")), "body", 10_000)?.to_owned();
            sqlx::query("UPDATE comments SET body = $1 WHERE id = $2").bind(body).bind(comment_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        }
        transaction.commit().await.map_err(ApiError::internal)?;
        replace_card_mentions_with_roles(pool, card_id, None, "comment", comment_id, &existing_body, &mentioned_role_ids, &mentioned_user_ids).await?;
        let comment = load_card_comments(pool, card_id, None).await?.into_iter().find(|comment| comment.id == comment_id)
            .ok_or_else(|| ApiError::bad_request("Discord comment could not be loaded."))?;
        let _ = state.events.send(());
        return Ok(Json(comment));
    }

    for attachment in &attachment_rows {
        parts.push(discord_media_markdown(&attachment.filename, &attachment.media_type, &format!("/v1/attachments/{}/content", attachment.id)));
    }
    let body = valid_text(&parts.join("\n"), "body", 10_000)?.to_owned();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let comment_id = Uuid::new_v4();
    sqlx::query("INSERT INTO comments (id, card_id, author_id, body, external_author_name, external_author_avatar_url, discord_integration_id, discord_message_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7)")
        .bind(comment_id).bind(card_id).bind(&body).bind(&author_name).bind(&author_avatar_url).bind(integration.id).bind(&message_id)
        .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for attachment in attachment_rows {
        sqlx::query("INSERT INTO attachments (id, card_id, uploaded_by, object_key, original_name, media_type, byte_size, external_url, discord_channel_id, discord_message_id, discord_attachment_id) VALUES ($1, $2, NULL, NULL, $3, $4, $5, $6, $7, $8, $9)")
            .bind(attachment.id).bind(card_id).bind(attachment.filename).bind(attachment.media_type).bind(attachment.byte_size).bind(attachment.url).bind(attachment.channel_id).bind(attachment.message_id).bind(attachment.attachment_id)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    replace_card_mentions_with_roles(pool, card_id, None, "comment", comment_id, &body, &mentioned_role_ids, &mentioned_user_ids).await?;
    let comment = load_card_comments(pool, card_id, None).await?.into_iter().find(|comment| comment.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Discord comment could not be loaded."))?;
    record_external_card_activity(pool, card_id, "Discord: добавлен комментарий", &author_name).await;
    let _ = state.events.send(());
    Ok(Json(comment))
}

async fn update_comment(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>, Json(request): Json<UpdateCommentRequest>) -> ApiResult<CommentResponse> {
    let body = valid_text(&request.body, "body", 10_000)?;
    let pool = database(&state)?;
    let comment_card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
        .bind(comment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "comment_not_found", "Comment was not found.".to_owned()))?;
    ensure_card_permission(pool, comment_card_id, current.id, "edit_cards").await?;
    let card_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE comments c SET body = $1, edited_at = now() FROM cards card INNER JOIN boards b ON b.id = card.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $2 AND c.card_id = card.id AND c.author_id = $3 AND m.user_id = $3 AND card.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $3, 'edit_cards'::workspace_permission) RETURNING c.card_id",
    )
    .bind(&body)
    .bind(comment_id)
    .bind(current.id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::forbidden("Only the comment author can edit it."))?;
    replace_card_mentions(database(&state)?, card_id, current.id, "comment", comment_id, &body).await?;
    record_card_activity(database(&state)?, card_id, current.id, "Изменён комментарий", "").await;
    let comment = load_card_comments(database(&state)?, card_id, Some(current.id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    let _ = state.events.send(());
    Ok(Json(comment))
}

async fn toggle_comment_reaction(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>, Json(request): Json<ToggleCommentReactionRequest>) -> ApiResult<Vec<CommentReactionResponse>> {
    let emoji = request.emoji.trim();
    if emoji.is_empty() || emoji.chars().count() > 64 { return Err(ApiError::bad_request("Reaction must be between 1 and 64 characters.")); }
    let pool = database(&state)?;
    let (card_id, board_id) = sqlx::query_as::<_, (Uuid, Uuid)>("SELECT c.card_id, card.board_id FROM comments c INNER JOIN cards card ON card.id = c.card_id WHERE c.id = $1")
        .bind(comment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Comment was not found."))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if let Some(sticker_id) = emoji.strip_prefix("sticker:") {
        let sticker_id = Uuid::parse_str(sticker_id).map_err(|_| ApiError::bad_request("Sticker reaction id is invalid."))?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM board_stickers WHERE id = $1 AND board_id = $2)")
            .bind(sticker_id).bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !exists { return Err(ApiError::bad_request("Sticker does not belong to this project.")); }
    } else if emoji.chars().count() > 16 {
        return Err(ApiError::bad_request("Reaction emoji must be between 1 and 16 characters."));
    }
    let removed = sqlx::query("DELETE FROM comment_reactions WHERE comment_id = $1 AND user_id = $2 AND emoji = $3")
        .bind(comment_id).bind(current.id).bind(emoji).execute(pool).await.map_err(ApiError::internal)?;
    if removed.rows_affected() == 0 {
        sqlx::query("INSERT INTO comment_reactions (comment_id, user_id, emoji) VALUES ($1, $2, $3)")
            .bind(comment_id).bind(current.id).bind(emoji).execute(pool).await.map_err(ApiError::internal)?;
    }
    let comment = load_card_comments(pool, card_id, Some(current.id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    record_card_activity(pool, card_id, current.id, "Изменена реакция на комментарий", emoji).await;
    let _ = state.events.send(());
    Ok(Json(comment.reactions))
}

async fn delete_comment(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let comment_card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
        .bind(comment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "comment_not_found", "Comment was not found.".to_owned()))?;
    ensure_card_permission(pool, comment_card_id, current.id, "edit_cards").await?;
    let card_id = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM comments c USING cards card, boards b, board_members m WHERE c.id = $1 AND c.card_id = card.id AND card.board_id = b.id AND m.board_id = b.id AND m.user_id = $2 AND c.author_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) RETURNING c.card_id",
    )
    .bind(comment_id)
    .bind(current.id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::forbidden("You cannot delete this comment."))?;
    record_card_activity(database(&state)?, card_id, current.id, "Удалён комментарий", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_attachment(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<AttachmentResponse> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, actor_id).await?;
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Attachment form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Attachment file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Attachment field must be named file.")); }
    let original_name = field.file_name().unwrap_or("attachment").replace(['/', '\\'], "_");
    if original_name.is_empty() || original_name.chars().count() > 255 { return Err(ApiError::bad_request("Attachment filename must contain 1 to 255 characters.")); }
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_else(|| "application/octet-stream".to_owned());
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Only supported image, video, and audio files may be attached."))?;
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Attachment upload could not be read."))?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 { return Err(ApiError::bad_request("Attachment must be between 1 byte and 50 MiB.")); }
    let attachment_id = Uuid::new_v4();
    let object_key = format!("{attachment_id}.{extension}");
    let path = state.upload_dir.join(&object_key);
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "attachment write failed"); ApiError::storage() })?;
    let attachment = sqlx::query_as::<_, AttachmentResponse>(
        "INSERT INTO attachments (id, card_id, uploaded_by, object_key, original_name, media_type, byte_size) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS url",
    )
    .bind(attachment_id)
    .bind(card_id)
    .bind(actor_id)
    .bind(&object_key)
    .bind(&original_name)
    .bind(&media_type)
    .bind(bytes.len() as i64)
    .fetch_one(pool)
    .await;
    match attachment {
        Ok(attachment) => {
            record_card_activity(pool, card_id, actor_id, "Добавлено вложение", &original_name).await;
            let _ = state.events.send(());
            Ok(Json(attachment))
        },
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            Err(ApiError::internal(error))
        }
    }
}

async fn delete_attachment(State(state): State<AppState>, current: CurrentUser, Path(attachment_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let actor_id = current.id;
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM attachments WHERE id = $1")
        .bind(attachment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, actor_id, "edit_cards").await?;
    let object_key = sqlx::query_scalar::<_, Option<String>>(
        "DELETE FROM attachments a USING cards c, boards b, board_members m WHERE a.id = $1 AND a.card_id = c.id AND c.board_id = b.id AND c.archived_at IS NULL AND m.board_id = b.id AND m.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) RETURNING a.object_key",
    )
    .bind(attachment_id)
    .bind(actor_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment was not found.".to_owned()))?;
    if let Some(object_key) = object_key {
        let path = state.upload_dir.join(object_key);
        if let Err(error) = tokio::fs::remove_file(path).await {
            if error.kind() != std::io::ErrorKind::NotFound { tracing::error!(?error, "attachment file removal failed"); }
        }
    }
    record_card_activity(pool, card_id, actor_id, "Удалено вложение", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn download_attachment(State(state): State<AppState>, current: Viewer, Path(attachment_id): Path<Uuid>) -> Result<Response, ApiError> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM attachments WHERE id = $1")
    .bind(attachment_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment was not found.".to_owned()))?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    let attachment = sqlx::query_as::<_, AttachmentDownloadRecord>(
        "SELECT a.object_key, a.media_type, a.external_url, a.discord_channel_id, COALESCE(a.discord_message_id, source.discord_message_id) AS discord_message_id, a.discord_attachment_id, COALESCE(c.discord_integration_id, source.discord_integration_id) AS discord_integration_id FROM attachments a INNER JOIN cards c ON c.id = a.card_id LEFT JOIN LATERAL (SELECT cm.discord_integration_id, cm.discord_message_id FROM comments cm WHERE cm.card_id = c.id AND cm.discord_integration_id IS NOT NULL AND cm.body LIKE '%' || '/v1/attachments/' || a.id::text || '/content' || '%' ORDER BY cm.created_at DESC LIMIT 1) source ON TRUE WHERE a.id = $1",
    )
    .bind(attachment_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment was not found.".to_owned()))?;
    if let Some(url) = &attachment.external_url {
        return proxy_external_attachment_with_refresh(&state, attachment_id, &attachment, url).await;
    }
    let object_key = attachment.object_key.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(object_key)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found.".to_owned())
        } else {
            tracing::error!(?error, "attachment read failed");
            ApiError::storage()
        }
    })?;
    let content_type = HeaderValue::from_str(&attachment.media_type).map_err(|_| ApiError::storage())?;
    Ok(([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"))], bytes).into_response())
}

async fn upload_checklist_item_attachment(State(state): State<AppState>, current: CurrentUser, Path(item_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<AttachmentResponse> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT i.card_id FROM checklist_items i INNER JOIN cards c ON c.id = i.card_id INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members bm ON bm.board_id = b.id WHERE i.id = $1 AND c.archived_at IS NULL AND bm.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission)",
    )
    .bind(item_id).bind(current.id).fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Attachment form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Attachment file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Attachment field must be named file.")); }
    let original_name = field.file_name().unwrap_or("checklist-attachment").replace(['/', '\\'], "_");
    if original_name.is_empty() || original_name.chars().count() > 255 { return Err(ApiError::bad_request("Attachment filename must contain 1 to 255 characters.")); }
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_else(|| "application/octet-stream".to_owned());
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Only supported image, video, and audio files may be attached."))?;
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Attachment upload could not be read."))?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 { return Err(ApiError::bad_request("Attachment must be between 1 byte and 50 MiB.")); }
    let attachment_id = Uuid::new_v4();
    let object_key = format!("{attachment_id}.{extension}");
    let path = state.upload_dir.join(&object_key);
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "checklist attachment write failed"); ApiError::storage() })?;
    let attachment = sqlx::query_as::<_, AttachmentResponse>(
        "INSERT INTO attachments (id, card_id, checklist_item_id, uploaded_by, object_key, original_name, media_type, byte_size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id, original_name, media_type, byte_size, '/v1/attachments/' || id::text || '/content' AS url",
    )
    .bind(attachment_id).bind(card_id).bind(item_id).bind(current.id).bind(&object_key).bind(&original_name).bind(&media_type).bind(bytes.len() as i64)
    .fetch_one(pool).await;
    match attachment {
        Ok(attachment) => {
            record_card_activity(pool, card_id, current.id, "Добавлено вложение к пункту чек-листа", &original_name).await;
            let _ = state.events.send(());
            Ok(Json(attachment))
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            Err(ApiError::internal(error))
        }
    }
}

// External attachments stay remote: Flowboard streams them only after checking
// card access. The host allow-list prevents this from becoming a generic proxy.
async fn proxy_external_attachment(client: &reqwest::Client, url: &str, media_type: &str) -> Result<Response, ApiError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| ApiError::storage())?;
    if !is_external_attachment_url(&parsed) {
        return Err(ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "External attachment was not found.".to_owned()));
    }
    let response = client.get(parsed).send().await.map_err(|error| {
        tracing::warn!(?error, "external Discord attachment request failed");
        ApiError(StatusCode::BAD_GATEWAY, "attachment_unavailable", "External attachment is temporarily unavailable.".to_owned())
    })?;
    if !response.status().is_success() {
        return Err(ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "External attachment was not found.".to_owned()));
    }
    if response.content_length().is_some_and(|size| size > 50 * 1024 * 1024) {
        return Err(ApiError::bad_request("External attachment exceeds the 50 MiB limit."));
    }
    let content_type = HeaderValue::from_str(media_type).map_err(|_| ApiError::storage())?;
    let mut received = 0usize;
    let stream = response.bytes_stream().map(move |chunk| {
        chunk.map_err(std::io::Error::other).and_then(|bytes| {
            received += bytes.len();
            if received > 50 * 1024 * 1024 {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "External attachment exceeds the 50 MiB limit."))
            } else {
                Ok(bytes)
            }
        })
    });
    Ok(([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"))], Body::from_stream(stream)).into_response())
}

async fn proxy_external_attachment_with_refresh(
    state: &AppState,
    attachment_id: Uuid,
    attachment: &AttachmentDownloadRecord,
    url: &str,
) -> Result<Response, ApiError> {
    match proxy_external_attachment(&state.external_http, url, &attachment.media_type).await {
        Ok(response) => Ok(response),
        Err(error) if error.1 == "attachment_not_found" => {
            let Some(refresh) = &state.discord_attachment_refresh else { return Err(error); };
            let (Some(integration_id), Some(channel_id), Some(message_id), Some(discord_attachment_id)) = (
                attachment.discord_integration_id,
                attachment.discord_channel_id.as_deref(),
                attachment.discord_message_id.as_deref(),
                attachment.discord_attachment_id.as_deref(),
            ) else { return Err(error); };
            let refreshed_url = refresh_discord_attachment_url(&state.external_http, refresh, integration_id, channel_id, message_id, discord_attachment_id).await?;
            sqlx::query("UPDATE attachments SET external_url = $1 WHERE id = $2 AND external_url = $3")
                .bind(&refreshed_url).bind(attachment_id).bind(url)
                .execute(database(state)?).await.map_err(ApiError::internal)?;
            proxy_external_attachment(&state.external_http, &refreshed_url, &attachment.media_type).await
        }
        Err(error) => Err(error),
    }
}

async fn refresh_discord_attachment_url(
    client: &reqwest::Client,
    refresh: &DiscordAttachmentRefresh,
    integration_id: Uuid,
    channel_id: &str,
    message_id: &str,
    attachment_id: &str,
) -> Result<String, ApiError> {
    let payload = serde_json::to_vec(&DiscordAttachmentRefreshRequest { integration_id, channel_id, message_id, attachment_id })
        .map_err(|_| ApiError::storage())?;
    let timestamp = Utc::now().timestamp().to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(refresh.signing_secret.as_bytes()).map_err(|_| ApiError::storage())?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(&payload);
    let signature: String = mac.finalize().into_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
    let response = client.post(refresh.endpoint.clone())
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-flowboard-timestamp", timestamp)
        .header("x-flowboard-signature", signature)
        .body(payload)
        .send().await.map_err(|error| {
            tracing::warn!(?error, "Discord attachment refresh request failed");
            ApiError(StatusCode::BAD_GATEWAY, "attachment_unavailable", "External attachment is temporarily unavailable.".to_owned())
        })?;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "Discord attachment refresh was rejected");
        return Err(ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "External attachment was not found.".to_owned()));
    }
    let bytes = response.bytes().await.map_err(|error| {
        tracing::warn!(?error, "Discord attachment refresh response could not be read");
        ApiError(StatusCode::BAD_GATEWAY, "attachment_unavailable", "External attachment is temporarily unavailable.".to_owned())
    })?;
    let refreshed: DiscordAttachmentRefreshResponse = serde_json::from_slice(&bytes).map_err(|error| {
        tracing::warn!(?error, "Discord attachment refresh response is invalid");
        ApiError(StatusCode::BAD_GATEWAY, "attachment_unavailable", "External attachment is temporarily unavailable.".to_owned())
    })?;
    valid_discord_asset_url(&refreshed.url).map(ToOwned::to_owned)
}

async fn create_card(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>, Json(request): Json<CreateCardRequest>) -> ApiResult<CardResponse> {
    ensure_list_permission(database(&state)?, list_id, current.id, "create_cards").await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 500)?;
    let description = request.description.trim();
    if description.chars().count() > 20_000 { return Err(ApiError::bad_request("description must not exceed 20000 characters.")); }
    let card = sqlx::query_as::<_, CardResponse>(
        "INSERT INTO cards (id, board_id, list_id, title, description, position, created_by) SELECT $1, l.board_id, l.id, $2, $3, COALESCE((SELECT MAX(position) FROM cards WHERE list_id = l.id), 0) + 1000, $4 FROM lists l INNER JOIN boards b ON b.id = l.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE l.id = $5 AND m.user_id = $4 RETURNING id, list_id, title, description, start_at::text AS start_at",
    )
    .bind(Uuid::new_v4()).bind(title).bind(description).bind(actor_id).bind(list_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    replace_card_mentions(database(&state)?, card.id, actor_id, "card_description", card.id, &card.description).await?;
    record_card_activity(database(&state)?, card.id, actor_id, "Создана задача", &card.title).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn update_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardRequest>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    // This endpoint is the sole escape hatch from a frozen card. Changing its
    // frozen state itself is administrative; all other changes wait for an
    // explicit unfreeze request.
    ensure_card_access_permission(pool, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let title = match request.title {
        Some(title) => Some(valid_text(&title, "title", 500)?.to_owned()),
        None => None,
    };
    let description = match request.description {
        Some(description) if description.chars().count() <= 20_000 => Some(description.trim().to_owned()),
        Some(_) => return Err(ApiError::bad_request("description must not exceed 20000 characters.")),
        None => None,
    };
    let priority = match request.priority {
        Some(value) if (0..=5).contains(&value) => Some(value),
        Some(_) => return Err(ApiError::bad_request("priority must be between 0 and 5.")),
        None => None,
    };
    let is_frozen = request.is_frozen;
    let start_at = match request.start_at {
        Some(Some(value)) => Some(Some(valid_due_at(&value)?)),
        Some(None) => Some(None),
        None => None,
    };
    if title.is_none() && description.is_none() && priority.is_none() && is_frozen.is_none() && start_at.is_none() { return Err(ApiError::bad_request("At least one editable field is required.")); }
    let currently_frozen = sqlx::query_scalar::<_, bool>("SELECT is_frozen FROM cards WHERE id = $1 AND archived_at IS NULL")
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Card was not found."))?;
    if is_frozen.is_some() {
        // Otherwise an ordinary editor could lock everybody else out of a card.
        ensure_card_full_access(pool, card_id, actor_id).await?;
    }
    if currently_frozen && (is_frozen != Some(false) || title.is_some() || description.is_some() || priority.is_some() || start_at.is_some()) {
        return Err(ApiError::forbidden("This card is frozen. Unfreeze it first, then make changes."));
    }
    let text_change_detail = match (title.is_some(), description.is_some()) {
        (true, true) => "Название и описание карточки",
        (true, false) => "Название карточки",
        (false, true) => "Описание карточки",
        (false, false) => "",
    };
    let description_changed = description.is_some();
    let priority_detail = priority.map(|value| if value == 0 { "Приоритет снят".to_owned() } else { format!("Приоритет: {value}/5") });
    let freeze_detail = is_frozen.map(|value| if value { "Карточка заморожена" } else { "Карточка разморожена" });
    if let Some(description) = description.as_deref() {
        sqlx::query("INSERT INTO card_description_versions (id, card_id, description, created_by) SELECT $1, c.id, c.description, $3 FROM cards c INNER JOIN boards b ON b.id = c.board_id LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $3 WHERE c.id = $2 AND c.archived_at IS NULL AND b.archived_at IS NULL AND c.description IS DISTINCT FROM $4 AND flowboard_has_permission(b.workspace_id, $3, 'edit_cards'::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $3 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $3 AND wm.role IN ('owner', 'full_access')))")
            .bind(Uuid::new_v4()).bind(card_id).bind(actor_id).bind(description).execute(database(&state)?).await.map_err(ApiError::internal)?;
    }
    let card = sqlx::query_as::<_, CardResponse>(
        "UPDATE cards c SET title = COALESCE($1, c.title), description = COALESCE($2, c.description), priority = COALESCE($3, c.priority), is_frozen = COALESCE($4, c.is_frozen), start_at = CASE WHEN $5 THEN $6 ELSE c.start_at END, updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $7 AND c.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $8 RETURNING c.id, c.list_id, c.title, c.description, c.start_at::text AS start_at",
    )
    .bind(title)
    .bind(description)
    .bind(priority)
    .bind(is_frozen)
    .bind(start_at.is_some())
    .bind(start_at.flatten())
    .bind(card_id)
    .bind(actor_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    if description_changed { replace_card_mentions(database(&state)?, card.id, actor_id, "card_description", card.id, &card.description).await?; }
    if priority_detail.is_some() {
        record_card_activity(database(&state)?, card.id, actor_id, "Изменена задача", priority_detail.as_deref().unwrap()).await;
    } else if let Some(detail) = freeze_detail {
        record_card_activity(database(&state)?, card.id, actor_id, detail, "").await;
    } else {
        record_card_edit_activity(database(&state)?, card.id, actor_id, text_change_detail).await;
    }
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn update_due_date(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateDueDateRequest>) -> Result<StatusCode, ApiError> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let due_at = valid_due_at(&request.due_at)?;
    let result = sqlx::query(
        "UPDATE cards c SET due_at = $1, updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $2 AND c.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $3",
    )
    .bind(due_at)
    .bind(card_id)
    .bind(actor_id)
    .execute(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(database(&state)?, card_id, actor_id, "Установлен дедлайн", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_due_date(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let result = sqlx::query(
        "UPDATE cards c SET due_at = NULL, updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $1 AND c.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $2",
    )
    .bind(card_id)
    .bind(actor_id)
    .execute(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(database(&state)?, card_id, actor_id, "Снят дедлайн", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_card_cover(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardCoverRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if request.mode != "full" && request.mode != "top" { return Err(ApiError::bad_request("Cover mode must be full or top.")); }
    if let Some(attachment_id) = request.attachment_id {
        let is_supported_cover: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attachments WHERE id = $1 AND card_id = $2 AND (media_type LIKE 'image/%' OR media_type LIKE 'video/%'))")
            .bind(attachment_id).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !is_supported_cover { return Err(ApiError::bad_request("Card cover must be an image or video attached to this card.")); }
    }
    let result = sqlx::query("UPDATE cards SET cover_attachment_id = $1, cover_mode = $2, updated_at = now() WHERE id = $3 AND archived_at IS NULL")
        .bind(request.attachment_id).bind(request.mode).bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(pool, card_id, current.id, if request.attachment_id.is_some() { "Установлена обложка" } else { "Снята обложка" }, "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_card_background(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardBackgroundRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let url = match request.background_image_url {
        Some(value) if !value.trim().is_empty() => {
            let value = value.trim();
            if value.len() > 2_000 || !(value.starts_with("https://") || value.starts_with("/v1/")) { return Err(ApiError::bad_request("Card background must be an HTTPS image URL or a Flowboard attachment.")); }
            Some(value.to_owned())
        }
        _ => None,
    };
    let uploaded_file_url = format!("/v1/cards/{card_id}/background/file");
    let result = sqlx::query("UPDATE cards SET background_image_url = $1, updated_at = now() WHERE id = $2 AND archived_at IS NULL")
        .bind(url.clone()).bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    if url.as_deref() != Some(uploaded_file_url.as_str()) {
        if let Some(object_key) = sqlx::query_scalar::<_, String>("DELETE FROM card_backgrounds WHERE card_id = $1 RETURNING object_key")
            .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
            let _ = tokio::fs::remove_file(state.upload_dir.join(object_key)).await;
        }
    }
    record_card_activity(pool, card_id, current.id, "Изменён фон карточки", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_card_background(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<BoardBackgroundUploadResponse> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let field = multipart.next_field().await.map_err(|_| ApiError::bad_request("Card background upload form is invalid."))?
        .ok_or_else(|| ApiError::bad_request("Card background image file is required."))?;
    if field.name() != Some("file") { return Err(ApiError::bad_request("Card background image field must be named file.")); }
    let original_name = field.file_name().unwrap_or("card-background").replace(['/', '\\'], "_");
    let media_type = field.content_type().map(ToString::to_string).unwrap_or_default();
    if !matches!(media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp") {
        return Err(ApiError::bad_request("Card background must be a JPEG, PNG, GIF, or WebP image."));
    }
    let bytes = field.bytes().await.map_err(|_| ApiError::bad_request("Card background image could not be read."))?;
    if bytes.is_empty() || bytes.len() > 50 * 1024 * 1024 { return Err(ApiError::bad_request("Card background must be between 1 byte and 50 MiB.")); }
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Card background image type is unsupported."))?;
    let object_key = format!("card-background-{}.{}", Uuid::new_v4(), extension);
    let path = state.upload_dir.join(&object_key);
    tokio::fs::write(&path, bytes.as_ref()).await.map_err(|error| { tracing::error!(?error, "card background write failed"); ApiError::storage() })?;

    let url = format!("/v1/cards/{card_id}/background/file");
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let previous_key = sqlx::query_scalar::<_, Option<String>>("SELECT object_key FROM card_backgrounds WHERE card_id = $1 FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?.flatten();
    let saved = sqlx::query("INSERT INTO card_backgrounds (card_id, uploaded_by, object_key, original_name, media_type, byte_size) SELECT c.id, $2, $3, $4, $5, $6 FROM cards c WHERE c.id = $1 AND c.archived_at IS NULL ON CONFLICT (card_id) DO UPDATE SET uploaded_by = EXCLUDED.uploaded_by, object_key = EXCLUDED.object_key, original_name = EXCLUDED.original_name, media_type = EXCLUDED.media_type, byte_size = EXCLUDED.byte_size, created_at = now()")
        .bind(card_id).bind(current.id).bind(&object_key).bind(&original_name).bind(&media_type).bind(bytes.len() as i64)
        .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    if saved.rows_affected() == 0 { let _ = tokio::fs::remove_file(&path).await; return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    sqlx::query("UPDATE cards SET background_image_url = $1, updated_at = now() WHERE id = $2")
        .bind(&url).bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    if let Some(previous_key) = previous_key { let _ = tokio::fs::remove_file(state.upload_dir.join(previous_key)).await; }
    record_card_activity(pool, card_id, current.id, "Изменён фон карточки", "Загружен файл").await;
    let _ = state.events.send(());
    Ok(Json(BoardBackgroundUploadResponse { url: format!("{url}?v={}", Uuid::new_v4()) }))
}

async fn download_card_background(State(state): State<AppState>, current: Viewer, Path(card_id): Path<Uuid>) -> Result<Response, ApiError> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    let background = sqlx::query_as::<_, (String, String)>("SELECT object_key, media_type FROM card_backgrounds WHERE card_id = $1")
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "background_not_found", "Card background was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(background.0)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "background_not_found", "Card background file was not found.".to_owned()) }
        else { tracing::error!(?error, "card background read failed"); ApiError::storage() }
    })?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&background.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"))], bytes).into_response())
}

// Public boards expose only the media that is visibly referenced by that board.
// This deliberately avoids making the generic avatar endpoint a public directory.
async fn download_public_board_background(State(state): State<AppState>, Path(board_id): Path<Uuid>) -> Result<Response, ApiError> {
    let background = sqlx::query_as::<_, (String, String)>("SELECT bb.object_key, bb.media_type FROM board_backgrounds bb INNER JOIN boards b ON b.id = bb.board_id WHERE bb.board_id = $1 AND b.archived_at IS NULL AND b.visibility = 'public'")
        .bind(board_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "background_not_found", "Board background was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(background.0)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "background_not_found", "Board background file was not found.".to_owned()) }
        else { tracing::error!(?error, "public board background read failed"); ApiError::storage() }
    })?;
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_str(&background.1).map_err(|_| ApiError::storage())?), (header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"))], bytes).into_response())
}

async fn download_public_board_avatar(State(state): State<AppState>, Path((board_id, user_id)): Path<(Uuid, Uuid)>) -> Result<Response, ApiError> {
    let is_visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM boards b WHERE b.id = $1 AND b.archived_at IS NULL AND b.visibility = 'public' AND (EXISTS(SELECT 1 FROM board_members bm WHERE bm.board_id = b.id AND bm.user_id = $2) OR EXISTS(SELECT 1 FROM card_assignees ca INNER JOIN cards c ON c.id = ca.card_id WHERE c.board_id = b.id AND ca.user_id = $2) OR EXISTS(SELECT 1 FROM comments cm INNER JOIN cards c ON c.id = cm.card_id WHERE c.board_id = b.id AND cm.author_id = $2)))")
        .bind(board_id).bind(user_id).fetch_one(database(&state)?).await.map_err(ApiError::internal)?;
    if !is_visible { return Err(ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned())); }
    avatar_response(&state, user_id).await
}

async fn discord_public_media_token(pool: &PgPool, integration_id: Uuid) -> Result<Uuid, ApiError> {
    if let Some(token) = sqlx::query_scalar::<_, Uuid>("SELECT token FROM discord_public_media_tokens WHERE integration_id = $1")
        .bind(integration_id).fetch_optional(pool).await.map_err(ApiError::internal)?
    {
        return Ok(token);
    }
    let generated = Uuid::new_v4();
    sqlx::query("INSERT INTO discord_public_media_tokens (integration_id, token) VALUES ($1, $2) ON CONFLICT (integration_id) DO NOTHING")
        .bind(integration_id).bind(generated).execute(pool).await.map_err(ApiError::internal)?;
    sqlx::query_scalar::<_, Uuid>("SELECT token FROM discord_public_media_tokens WHERE integration_id = $1")
        .bind(integration_id).fetch_one(pool).await.map_err(ApiError::internal)
}

async fn rewrite_discord_comment_avatar_urls(pool: &PgPool, integration: DiscordIntegration, card_id: Uuid, comments: &mut [CommentResponse]) -> Result<(), ApiError> {
    if !comments.iter().any(|comment| comment.author_id.is_some() && comment.author_avatar_url.as_deref().is_some_and(|url| url.starts_with("/v1/avatars/"))) {
        return Ok(());
    }
    let token = discord_public_media_token(pool, integration.id).await?;
    for comment in comments {
        if let (Some(author_id), Some(url)) = (comment.author_id, comment.author_avatar_url.as_deref()) {
            if url.starts_with("/v1/avatars/") {
                comment.author_avatar_url = Some(format!("/v1/discord-media/{token}/cards/{card_id}/avatars/{author_id}"));
            }
        }
    }
    Ok(())
}

fn rewrite_discord_comment_attachment_urls(_integration: DiscordIntegration, card_id: Uuid, comments: &mut [CommentResponse]) {
    for comment in comments {
        for attachment in &mut comment.attachments {
            attachment.download_url = format!("/v1/integrations/discord/cards/{card_id}/attachments/{}", attachment.id);
        }
    }
}

// Voice messages are private to Flowboard. Do this at the outbound API
// boundary so every Discord bot implementation receives neither the file nor
// the Flowboard attachment URL, while the original comment remains intact.
fn remove_discord_outbound_voice_messages(comments: &mut [CommentResponse]) {
    for comment in comments {
        let voice_attachment_ids: Vec<Uuid> = comment.attachments.iter()
            .filter(|attachment| attachment.media_type.starts_with("audio/"))
            .map(|attachment| attachment.id)
            .collect();
        if voice_attachment_ids.is_empty() { continue; }
        comment.attachments.retain(|attachment| !attachment.media_type.starts_with("audio/"));
        for attachment_id in voice_attachment_ids {
            comment.body = remove_audio_attachment_markdown(&comment.body, attachment_id);
        }
    }
}

fn remove_audio_attachment_markdown(body: &str, attachment_id: Uuid) -> String {
    let suffix = format!("](/v1/attachments/{attachment_id}/content)");
    let mut rendered = String::with_capacity(body.len());
    let mut remainder = body;
    while let Some(end) = remainder.find(&suffix) {
        let before = &remainder[..end];
        if let Some(start) = before.rfind("![audio:") {
            rendered.push_str(&before[..start]);
            remainder = &remainder[end + suffix.len()..];
        } else {
            let keep = end + suffix.len();
            rendered.push_str(&remainder[..keep]);
            remainder = &remainder[keep..];
        }
    }
    rendered.push_str(remainder);
    rendered
}

// `[[sticker:😀]]` is an internal composer marker: Flowboard turns it into a
// sticker while editing, but Discord only sees literal text. The integration
// API is an outbound boundary, so expose the native emoji there instead.
fn discord_outbound_comment_body(body: &str) -> String {
    const PREFIX: &str = "[[sticker:";
    let mut rendered = String::with_capacity(body.len());
    let mut remainder = body;
    while let Some(start) = remainder.find(PREFIX) {
        rendered.push_str(&remainder[..start]);
        let candidate = &remainder[start + PREFIX.len()..];
        let Some(end) = candidate.find("]]") else {
            rendered.push_str(PREFIX);
            rendered.push_str(candidate);
            return rendered;
        };
        let emoji = &candidate[..end];
        if emoji.is_empty() || emoji.contains(['\r', '\n']) || emoji.chars().count() > 16 {
            rendered.push_str(PREFIX);
            remainder = candidate;
            continue;
        }
        rendered.push_str(emoji);
        remainder = &candidate[end + 2..];
    }
    rendered.push_str(remainder);
    rendered
}

fn rewrite_discord_outbound_comment_bodies(comments: &mut [CommentResponse]) {
    for comment in comments {
        comment.body = discord_outbound_comment_body(&comment.body);
    }
}

async fn download_discord_card_attachment(State(state): State<AppState>, integration: DiscordIntegration, Path((card_id, attachment_id)): Path<(Uuid, Uuid)>) -> Result<Response, ApiError> {
    let attachment = sqlx::query_as::<_, AttachmentDownloadRecord>(
        "SELECT a.object_key, a.media_type, a.external_url, a.discord_channel_id, COALESCE(a.discord_message_id, source.discord_message_id) AS discord_message_id, a.discord_attachment_id, COALESCE(c.discord_integration_id, source.discord_integration_id) AS discord_integration_id FROM attachments a INNER JOIN cards c ON c.id = a.card_id INNER JOIN LATERAL (SELECT cm.discord_integration_id, cm.discord_message_id FROM comments cm WHERE cm.card_id = c.id AND cm.discord_integration_id IS NOT NULL AND cm.body LIKE '%' || '/v1/attachments/' || a.id::text || '/content' || '%' ORDER BY cm.created_at DESC LIMIT 1) source ON TRUE WHERE a.id = $1 AND c.id = $2 AND c.board_id = $3 AND c.archived_at IS NULL",
    )
    .bind(attachment_id).bind(card_id).bind(integration.board_id)
    .fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Comment attachment was not found on this Discord integration board.".to_owned()))?;
    if let Some(url) = &attachment.external_url {
        return proxy_external_attachment_with_refresh(&state, attachment_id, &attachment, url).await;
    }
    let object_key = attachment.object_key.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found on this Discord integration board.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(object_key)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found.".to_owned()) }
        else { tracing::error!(?error, "Discord attachment read failed"); ApiError::storage() }
    })?;
    let content_type = HeaderValue::from_str(&attachment.media_type).map_err(|_| ApiError::storage())?;
    Ok(([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"))], bytes).into_response())
}

async fn download_discord_comment_avatar(State(state): State<AppState>, Path((token, card_id, user_id)): Path<(Uuid, Uuid, Uuid)>) -> Result<Response, ApiError> {
    let is_visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM discord_public_media_tokens mt INNER JOIN discord_integrations di ON di.id = mt.integration_id INNER JOIN boards b ON b.id = di.board_id INNER JOIN cards c ON c.id = $2 AND c.board_id = b.id WHERE mt.token = $1 AND di.revoked_at IS NULL AND b.archived_at IS NULL AND EXISTS(SELECT 1 FROM comments cm WHERE cm.card_id = c.id AND cm.author_id = $3))")
        .bind(token).bind(card_id).bind(user_id).fetch_one(database(&state)?).await.map_err(ApiError::internal)?;
    if !is_visible { return Err(ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned())); }
    avatar_response(&state, user_id).await
}

async fn list_my_tasks(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<MyTaskResponse>> {
    let tasks = sqlx::query_as::<_, MyTaskResponse>(
        "SELECT c.id, c.board_id, b.title AS board_title, l.title AS list_title, c.title, c.priority, c.due_at::text AS due_at, c.completed_at::text AS completed_at, c.updated_at::text AS updated_at, \
         ARRAY_REMOVE(ARRAY[ \
           CASE WHEN EXISTS(SELECT 1 FROM card_assignees ca WHERE ca.card_id = c.id AND ca.user_id = $1) THEN 'Исполнитель' END, \
           CASE WHEN EXISTS(SELECT 1 FROM card_profile_roles cpr INNER JOIN user_profile_roles upr ON upr.role_id = cpr.role_id WHERE cpr.card_id = c.id AND upr.user_id = $1) THEN 'Роль' END, \
           CASE WHEN EXISTS(SELECT 1 FROM card_waiting_for cw WHERE cw.card_id = c.id AND cw.user_id = $1) THEN 'Ожидание' END, \
           CASE WHEN EXISTS(SELECT 1 FROM card_waiting_for cw INNER JOIN user_profile_roles upr ON upr.role_id = cw.role_id WHERE cw.card_id = c.id AND upr.user_id = $1) THEN 'Ожидание роли' END, \
           CASE WHEN EXISTS(SELECT 1 FROM card_reviewers cr WHERE cr.card_id = c.id AND cr.user_id = $1) THEN 'Проверка' END \
         ], NULL::text) AS reasons \
         FROM cards c \
         INNER JOIN boards b ON b.id = c.board_id \
         INNER JOIN lists l ON l.id = c.list_id \
         WHERE c.archived_at IS NULL AND b.archived_at IS NULL \
           AND ( \
             EXISTS(SELECT 1 FROM card_assignees ca WHERE ca.card_id = c.id AND ca.user_id = $1) \
             OR EXISTS(SELECT 1 FROM card_profile_roles cpr INNER JOIN user_profile_roles upr ON upr.role_id = cpr.role_id WHERE cpr.card_id = c.id AND upr.user_id = $1) \
             OR EXISTS(SELECT 1 FROM card_waiting_for cw WHERE cw.card_id = c.id AND cw.user_id = $1) \
             OR EXISTS(SELECT 1 FROM card_waiting_for cw INNER JOIN user_profile_roles upr ON upr.role_id = cw.role_id WHERE cw.card_id = c.id AND upr.user_id = $1) \
             OR EXISTS(SELECT 1 FROM card_reviewers cr WHERE cr.card_id = c.id AND cr.user_id = $1) \
           ) \
           AND ( \
             (b.visibility = 'public' AND c.is_public) \
             OR EXISTS(SELECT 1 FROM users u WHERE u.id = $1 AND u.is_system_owner AND u.disabled_at IS NULL) \
             OR EXISTS(SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $1 AND wm.role IN ('owner', 'full_access')) \
             OR EXISTS(SELECT 1 FROM board_members bm INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = b.id AND bm.user_id = $1 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END >= CASE c.min_view_preset WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 0 END) \
           ) \
         ORDER BY c.completed_at IS NULL DESC, c.due_at NULLS LAST, c.updated_at DESC \
         LIMIT 300",
    )
    .bind(current.id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(tasks))
}

async fn load_card_review(pool: &PgPool, card_id: Uuid) -> Result<CardReviewResponse, ApiError> {
    let review = sqlx::query_as::<_, CardReviewRow>(
        "SELECT cr.status, cr.updated_at::text AS updated_at, cr.requested_by, requester.username AS requested_by_name, \
         CASE WHEN requester.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || requester.id::text END AS requested_by_avatar_url \
         FROM card_reviews cr LEFT JOIN users requester ON requester.id = cr.requested_by WHERE cr.card_id = $1",
    )
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    let decisions = sqlx::query_as::<_, CardReviewDecisionResponse>(
        "SELECT reviewer.id AS reviewer_id, reviewer.username AS reviewer_name, \
         CASE WHEN reviewer.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || reviewer.id::text END AS reviewer_avatar_url, \
         decision.status, NULLIF(decision.reason, '') AS reason, decision.decided_at::text AS decided_at \
         FROM card_reviewers assigned \
         INNER JOIN users reviewer ON reviewer.id = assigned.user_id \
         LEFT JOIN card_review_decisions decision ON decision.card_id = assigned.card_id AND decision.reviewer_id = assigned.user_id \
         WHERE assigned.card_id = $1 ORDER BY reviewer.username COLLATE \"C\"",
    )
    .bind(card_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let reviewers = decisions.iter().map(|decision| MemberResponse {
        id: decision.reviewer_id,
        display_name: decision.reviewer_name.clone(),
        avatar_url: decision.reviewer_avatar_url.clone(),
    }).collect();
    Ok(CardReviewResponse {
        status: review.as_ref().map(|item| item.status.clone()).unwrap_or_else(|| "none".to_owned()),
        reviewers,
        decisions,
        requested_by: review.as_ref().and_then(|item| Some(MemberResponse {
            id: item.requested_by?,
            display_name: item.requested_by_name.clone()?,
            avatar_url: item.requested_by_avatar_url.clone(),
        })),
        updated_at: review.map(|item| item.updated_at),
    })
}

async fn get_card_review(State(state): State<AppState>, current: Viewer, Path(card_id): Path<Uuid>) -> ApiResult<CardReviewResponse> {
    let pool = database(&state)?;
    ensure_card_public_read(pool, card_id, current.0.map(|user| user.id)).await?;
    Ok(Json(load_card_review(pool, card_id).await?))
}

async fn update_card_review(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardReviewRequest>) -> ApiResult<CardReviewResponse> {
    if !matches!(request.status.as_str(), "none" | "requested") {
        return Err(ApiError::bad_request("A review can only be requested or cancelled here."));
    }
    if request.reviewer_ids.len() > 30 {
        return Err(ApiError::bad_request("a card can have at most 30 reviewers."));
    }
    let mut reviewer_ids = request.reviewer_ids;
    reviewer_ids.sort(); reviewer_ids.dedup();
    if request.status == "requested" && reviewer_ids.is_empty() {
        return Err(ApiError::bad_request("Choose at least one reviewer before requesting a review."));
    }
    if request.status == "none" && !reviewer_ids.is_empty() {
        return Err(ApiError::bad_request("A cancelled review cannot keep reviewers."));
    }
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if !reviewer_ids.is_empty() {
        let found: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM board_members bm \
             WHERE bm.board_id = (SELECT board_id FROM cards WHERE id = $1) AND bm.user_id = ANY($2)",
        )
        .bind(card_id).bind(&reviewer_ids).fetch_one(pool).await.map_err(ApiError::internal)?;
        if found != reviewer_ids.len() as i64 {
            return Err(ApiError::bad_request("every reviewer must be a member of this board."));
        }
    }
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let existed = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM card_reviews WHERE card_id = $1)")
        .bind(card_id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    if request.status == "none" {
        sqlx::query("DELETE FROM card_review_decisions WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM card_reviewers WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM card_reviews WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    } else {
        sqlx::query("INSERT INTO card_reviews (card_id, status, updated_by, requested_by, updated_at) VALUES ($1, 'requested', $2, $2, now()) ON CONFLICT (card_id) DO UPDATE SET status = 'requested', updated_by = EXCLUDED.updated_by, requested_by = EXCLUDED.requested_by, updated_at = now()")
            .bind(card_id).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM card_review_decisions WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM card_reviewers WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        for reviewer_id in &reviewer_ids {
            sqlx::query("INSERT INTO card_reviewers (card_id, user_id) VALUES ($1, $2)").bind(card_id).bind(reviewer_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        }
        sqlx::query("UPDATE cards SET completed_at = NULL, updated_at = now() WHERE id = $1 AND archived_at IS NULL")
            .bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    if request.status == "requested" {
        clear_card_review_request_notifications(pool, card_id).await;
        record_card_activity(pool, card_id, current.id, if existed { "Проверка запрошена повторно" } else { "Запрошена проверка" }, &format!("Проверяющих: {}", reviewer_ids.len())).await;
        notify_card_reviewers(pool, card_id, current.id, &reviewer_ids).await;
    } else if existed {
        clear_card_review_request_notifications(pool, card_id).await;
        record_card_activity(pool, card_id, current.id, "Проверка отменена", "").await;
    }
    let result = load_card_review(pool, card_id).await?;
    let _ = state.events.send(());
    Ok(Json(result))
}

async fn decide_card_review(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<DecideCardReviewRequest>) -> ApiResult<CardReviewResponse> {
    if !matches!(request.status.as_str(), "approved" | "changes_requested" | "rejected") {
        return Err(ApiError::bad_request("review decision is invalid."));
    }
    let reason = request.reason.trim();
    if matches!(request.status.as_str(), "changes_requested" | "rejected") && reason.is_empty() {
        return Err(ApiError::bad_request("Explain why changes are needed or the review is rejected."));
    }
    if reason.chars().count() > 4_000 { return Err(ApiError::bad_request("Review reason must be at most 4000 characters.")); }
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let review = sqlx::query_as::<_, (String, Option<Uuid>)>("SELECT status, requested_by FROM card_reviews WHERE card_id = $1 FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("No review is requested for this card."))?;
    if review.0 != "requested" { return Err(ApiError::bad_request("This review is no longer awaiting decisions.")); }
    let assigned = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM card_reviewers WHERE card_id = $1 AND user_id = $2)")
        .bind(card_id).bind(current.id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    if !assigned { return Err(ApiError::forbidden("Only an assigned reviewer can submit this decision.")); }
    sqlx::query("INSERT INTO card_review_decisions (card_id, reviewer_id, status, reason, decided_at) VALUES ($1, $2, $3, $4, now()) ON CONFLICT (card_id, reviewer_id) DO UPDATE SET status = EXCLUDED.status, reason = EXCLUDED.reason, decided_at = now()")
        .bind(card_id).bind(current.id).bind(&request.status).bind(reason).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    let (reviewer_count, decision_count) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT (SELECT COUNT(*) FROM card_reviewers WHERE card_id = $1), (SELECT COUNT(*) FROM card_review_decisions WHERE card_id = $1)",
    )
    .bind(card_id).fetch_one(&mut *transaction).await.map_err(ApiError::internal)?;
    let final_status = if reviewer_count > 0 && reviewer_count == decision_count {
        let decisions = sqlx::query_scalar::<_, String>("SELECT status FROM card_review_decisions WHERE card_id = $1")
            .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
        Some(if decisions.iter().any(|status| status == "rejected") { "rejected" } else if decisions.iter().any(|status| status == "changes_requested") { "changes_requested" } else { "approved" })
    } else { None };
    let final_reason = if let Some(status) = final_status {
        let reasons = sqlx::query_scalar::<_, String>("SELECT reason FROM card_review_decisions WHERE card_id = $1 AND reason <> '' ORDER BY decided_at")
            .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("UPDATE card_reviews SET status = $2, updated_by = $3, updated_at = now() WHERE card_id = $1")
            .bind(card_id).bind(status).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        sqlx::query("UPDATE cards SET completed_at = CASE WHEN $2 = 'approved' THEN now() ELSE NULL END, updated_at = now() WHERE id = $1 AND archived_at IS NULL")
            .bind(card_id).bind(status).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        Some(reasons.join(" · "))
    } else { None };
    transaction.commit().await.map_err(ApiError::internal)?;
    if let Some(status) = final_status {
        clear_card_review_request_notifications(pool, card_id).await;
        let detail = final_reason.as_deref().filter(|detail| !detail.is_empty()).unwrap_or_else(|| match status { "approved" => "Все проверяющие одобрили работу", "changes_requested" => "Все проверяющие оставили решения", _ => "Все проверяющие завершили review" });
        let action = match status { "approved" => "Проверка одобрена", "changes_requested" => "По проверке нужны правки", "rejected" => "Проверка отклонена", _ => unreachable!() };
        record_card_activity(pool, card_id, current.id, action, detail).await;
        if status == "approved" { record_discord_card_thread_event(pool, card_id, "completed").await; }
        if let Some(requester_id) = review.1 { notify_review_requester(pool, card_id, requester_id, current.id, status, detail).await; }
    }
    let result = load_card_review(pool, card_id).await?;
    let _ = state.events.send(());
    Ok(Json(result))
}

fn card_relation_type_label(relation_type: &str) -> &'static str {
    match relation_type {
        "blocks" => "Блокирует",
        "depends_on" => "Зависит от",
        "duplicate" => "Дубликат",
        "related" => "Связана с",
        "part_of" => "Является частью",
        _ => "Связь",
    }
}

fn relation_dependency_direction(source_card_id: Uuid, target_card_id: Uuid, relation_type: &str) -> Option<(Uuid, Uuid)> {
    match relation_type {
        "blocks" => Some((source_card_id, target_card_id)),
        "depends_on" => Some((target_card_id, source_card_id)),
        _ => None,
    }
}

async fn dependency_cycle_path(pool: &PgPool, prerequisite_card_id: Uuid, dependent_card_id: Uuid) -> Result<Option<Vec<Uuid>>, ApiError> {
    sqlx::query_scalar::<_, Vec<Uuid>>(
        "WITH RECURSIVE edges AS (SELECT CASE WHEN relation_type = 'depends_on' THEN target_card_id ELSE source_card_id END AS from_card_id, CASE WHEN relation_type = 'depends_on' THEN source_card_id ELSE target_card_id END AS to_card_id FROM card_relations WHERE relation_type IN ('blocks', 'depends_on')), walk(card_id, path) AS (SELECT e.to_card_id, ARRAY[e.from_card_id, e.to_card_id] FROM edges e WHERE e.from_card_id = $1 UNION ALL SELECT e.to_card_id, w.path || e.to_card_id FROM edges e INNER JOIN walk w ON e.from_card_id = w.card_id WHERE NOT e.to_card_id = ANY(w.path)) SELECT path FROM walk WHERE card_id = $2 LIMIT 1",
    )
    .bind(dependent_card_id)
    .bind(prerequisite_card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

// `source_card_id` is the implementation card and `target_card_id` is its
// parent. Keep that hierarchy acyclic independently from blocking relations:
// being part of a patch note must never block a card from being completed.
async fn part_of_cycle_path(pool: &PgPool, parent_card_id: Uuid, child_card_id: Uuid) -> Result<Option<Vec<Uuid>>, ApiError> {
    sqlx::query_scalar::<_, Vec<Uuid>>(
        "WITH RECURSIVE walk(card_id, path) AS (SELECT r.target_card_id, ARRAY[r.source_card_id, r.target_card_id] FROM card_relations r WHERE r.source_card_id = $1 AND r.relation_type = 'part_of' UNION ALL SELECT r.target_card_id, w.path || r.target_card_id FROM card_relations r INNER JOIN walk w ON r.source_card_id = w.card_id WHERE r.relation_type = 'part_of' AND NOT r.target_card_id = ANY(w.path)) SELECT path FROM walk WHERE card_id = $2 LIMIT 1",
    )
    .bind(parent_card_id)
    .bind(child_card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)
}

async fn list_card_relations(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<Vec<CardRelationResponse>> {
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    let relations = sqlx::query_as::<_, CardRelationResponse>(
        "SELECT r.id, r.relation_type, r.note, CASE WHEN r.source_card_id = $1 THEN 'outgoing' ELSE 'incoming' END AS direction, other.id AS other_card_id, other.title AS other_card_title, other.list_id AS other_card_list_id, other.completed_at::text AS other_card_completed_at, r.created_at::text AS created_at FROM card_relations r INNER JOIN cards other ON other.id = CASE WHEN r.source_card_id = $1 THEN r.target_card_id ELSE r.source_card_id END WHERE (r.source_card_id = $1 OR r.target_card_id = $1) AND other.archived_at IS NULL ORDER BY r.created_at DESC, r.id DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(relations))
}

async fn list_board_relations(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<BoardRelationResponse>> {
    let pool = database(&state)?;
    ensure_board_layout_access(pool, board_id, current.id).await?;
    let relations = sqlx::query_as::<_, BoardRelationResponse>(
        "SELECT r.id, r.source_card_id, r.target_card_id, r.relation_type, r.note, r.created_at::text AS created_at FROM card_relations r INNER JOIN cards source ON source.id = r.source_card_id INNER JOIN cards target ON target.id = r.target_card_id WHERE source.board_id = $1 AND target.board_id = $1 AND source.archived_at IS NULL AND target.archived_at IS NULL ORDER BY r.created_at ASC, r.id ASC",
    )
    .bind(board_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(relations))
}

async fn create_card_relation(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateCardRelationRequest>) -> ApiResult<CardRelationResponse> {
    if card_id == request.target_card_id { return Err(ApiError::bad_request("A card cannot be related to itself.")); }
    if !matches!(request.relation_type.as_str(), "blocks" | "depends_on" | "duplicate" | "related" | "part_of") {
        return Err(ApiError::bad_request("relation_type must be blocks, depends_on, duplicate, related, or part_of."));
    }
    let note = request.note.trim();
    if note.chars().count() > 500 { return Err(ApiError::bad_request("relation note must contain at most 500 characters.")); }
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    ensure_card_access(pool, request.target_card_id, current.id).await?;
    ensure_card_unfrozen(pool, request.target_card_id).await?;
    if let Some((prerequisite, dependent)) = relation_dependency_direction(card_id, request.target_card_id, &request.relation_type) {
        if let Some(path) = dependency_cycle_path(pool, prerequisite, dependent).await? {
            let titles = sqlx::query_as::<_, (Uuid, String)>("SELECT id, title FROM cards WHERE id = ANY($1)")
                .bind(&path).fetch_all(pool).await.map_err(ApiError::internal)?;
            let title_by_id: HashMap<Uuid, String> = titles.into_iter().collect();
            let mut loop_titles = path.iter().filter_map(|id| title_by_id.get(id).cloned()).collect::<Vec<_>>();
            if let Some(first) = loop_titles.first().cloned() { loop_titles.push(first); }
            let detail = if loop_titles.len() >= 2 { format!(": {}", loop_titles.join(" → ")) } else { String::new() };
            return Err(ApiError::bad_request(format!("Нельзя создать связь: она замкнёт цикл зависимостей{detail}.")));
        }
    }
    if request.relation_type == "part_of" {
        if let Some(path) = part_of_cycle_path(pool, request.target_card_id, card_id).await? {
            let titles = sqlx::query_as::<_, (Uuid, String)>("SELECT id, title FROM cards WHERE id = ANY($1)")
                .bind(&path).fetch_all(pool).await.map_err(ApiError::internal)?;
            let title_by_id: HashMap<Uuid, String> = titles.into_iter().collect();
            let mut loop_titles = path.iter().filter_map(|id| title_by_id.get(id).cloned()).collect::<Vec<_>>();
            if let Some(first) = loop_titles.first().cloned() { loop_titles.push(first); }
            let detail = if loop_titles.len() >= 2 { format!(": {}", loop_titles.join(" → ")) } else { String::new() };
            return Err(ApiError::bad_request(format!("Нельзя создать связь: она замкнёт цикл вложенности{detail}.")));
        }
    }
    let relation = sqlx::query_as::<_, CardRelationResponse>(
        "WITH inserted AS (INSERT INTO card_relations (id, source_card_id, target_card_id, relation_type, note, created_by) SELECT $1, source.id, target.id, $4, $5, $6 FROM cards source INNER JOIN cards target ON target.id = $3 AND target.board_id = source.board_id WHERE source.id = $2 AND source.archived_at IS NULL AND target.archived_at IS NULL ON CONFLICT (source_card_id, target_card_id, relation_type) DO NOTHING RETURNING id, relation_type, note, target_card_id, created_at) SELECT inserted.id, inserted.relation_type, inserted.note, 'outgoing' AS direction, target.id AS other_card_id, target.title AS other_card_title, target.list_id AS other_card_list_id, target.completed_at::text AS other_card_completed_at, inserted.created_at::text AS created_at FROM inserted INNER JOIN cards target ON target.id = inserted.target_card_id",
    )
    .bind(Uuid::new_v4())
    .bind(card_id)
    .bind(request.target_card_id)
    .bind(&request.relation_type)
    .bind(note)
    .bind(current.id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::CONFLICT, "relation_exists", "This card relation already exists or the cards are in different projects.".to_owned()))?;
    let detail = format!("{}: {}", card_relation_type_label(&relation.relation_type), relation.other_card_title);
    record_card_activity(pool, card_id, current.id, "Добавлена связь карточек", &detail).await;
    let _ = state.events.send(());
    Ok(Json(relation))
}

async fn update_card_relation(State(state): State<AppState>, current: CurrentUser, Path((card_id, relation_id)): Path<(Uuid, Uuid)>, Json(request): Json<UpdateCardRelationRequest>) -> ApiResult<CardRelationResponse> {
    let note = request.note.trim();
    if note.chars().count() > 500 { return Err(ApiError::bad_request("relation note must contain at most 500 characters.")); }
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let relation = sqlx::query_as::<_, CardRelationResponse>(
        "WITH updated AS (UPDATE card_relations r SET note = $1 WHERE r.id = $2 AND (r.source_card_id = $3 OR r.target_card_id = $3) RETURNING r.id, r.relation_type, r.note, r.source_card_id, r.target_card_id, r.created_at) SELECT updated.id, updated.relation_type, updated.note, CASE WHEN updated.source_card_id = $3 THEN 'outgoing' ELSE 'incoming' END AS direction, other.id AS other_card_id, other.title AS other_card_title, other.list_id AS other_card_list_id, other.completed_at::text AS other_card_completed_at, updated.created_at::text AS created_at FROM updated INNER JOIN cards other ON other.id = CASE WHEN updated.source_card_id = $3 THEN updated.target_card_id ELSE updated.source_card_id END",
    )
    .bind(note)
    .bind(relation_id)
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "relation_not_found", "Card relation was not found.".to_owned()))?;
    record_card_activity(pool, card_id, current.id, "Изменено пояснение связи", &relation.other_card_title).await;
    let _ = state.events.send(());
    Ok(Json(relation))
}

async fn delete_card_relation(State(state): State<AppState>, current: CurrentUser, Path((card_id, relation_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let removed = sqlx::query_as::<_, (String, Uuid, Uuid)>(
        "DELETE FROM card_relations WHERE id = $1 AND (source_card_id = $2 OR target_card_id = $2) RETURNING relation_type, source_card_id, target_card_id",
    )
    .bind(relation_id)
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "relation_not_found", "Card relation was not found.".to_owned()))?;
    let other_card_id = if removed.1 == card_id { removed.2 } else { removed.1 };
    let other_title = sqlx::query_scalar::<_, String>("SELECT title FROM cards WHERE id = $1")
        .bind(other_card_id).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or_else(|| "карточка".to_owned());
    let detail = format!("{}: {}", card_relation_type_label(&removed.0), other_title);
    record_card_activity(pool, card_id, current.id, "Удалена связь карточек", &detail).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_card_description_versions(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<Vec<CardDescriptionVersionResponse>> {
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    let versions = sqlx::query_as::<_, CardDescriptionVersionResponse>(
        "SELECT v.id, v.description, COALESCE(u.username, 'Deleted user') AS author_name, v.created_at::text AS created_at FROM card_description_versions v LEFT JOIN users u ON u.id = v.created_by WHERE v.card_id = $1 ORDER BY v.created_at DESC, v.id DESC LIMIT 100",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(versions))
}

async fn restore_card_description_version(State(state): State<AppState>, current: CurrentUser, Path((card_id, version_id)): Path<(Uuid, Uuid)>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let version_description = sqlx::query_scalar::<_, String>("SELECT description FROM card_description_versions WHERE id = $1 AND card_id = $2")
        .bind(version_id).bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "description_version_not_found", "Description version was not found.".to_owned()))?;
    let current_description = sqlx::query_scalar::<_, String>("SELECT description FROM cards WHERE id = $1 AND archived_at IS NULL FOR UPDATE")
        .bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    if current_description != version_description {
        sqlx::query("INSERT INTO card_description_versions (id, card_id, description, created_by) VALUES ($1, $2, $3, $4)")
            .bind(Uuid::new_v4()).bind(card_id).bind(current_description).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    let card = sqlx::query_as::<_, CardResponse>("UPDATE cards SET description = $1, updated_at = now() WHERE id = $2 AND archived_at IS NULL RETURNING id, list_id, title, description, start_at::text AS start_at")
        .bind(version_description).bind(card_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    transaction.commit().await.map_err(ApiError::internal)?;
    replace_card_mentions(pool, card_id, current.id, "card_description", card_id, &card.description).await?;
    record_card_activity(pool, card_id, current.id, "Восстановлена версия описания", "").await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn load_card_polls(pool: &PgPool, card_id: Uuid, viewer_id: Uuid) -> Result<Vec<CardPollResponse>, ApiError> {
    let polls = sqlx::query_as::<_, CardPollRow>("SELECT p.id, p.question, COALESCE(u.username, 'Deleted user') AS created_by, p.created_at::text AS created_at FROM card_polls p LEFT JOIN users u ON u.id = p.created_by WHERE p.card_id = $1 ORDER BY p.created_at DESC")
        .bind(card_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    if polls.is_empty() { return Ok(Vec::new()); }
    let poll_ids = polls.iter().map(|poll| poll.id).collect::<Vec<_>>();
    let options = sqlx::query_as::<_, CardPollOptionRow>("SELECT o.poll_id, o.id, o.title, COUNT(v.user_id) AS votes, COALESCE(BOOL_OR(v.user_id = $2), false) AS voted FROM card_poll_options o LEFT JOIN card_poll_votes v ON v.option_id = o.id WHERE o.poll_id = ANY($1) GROUP BY o.poll_id, o.id, o.title, o.position ORDER BY o.poll_id, o.position")
        .bind(&poll_ids).bind(viewer_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let mut options_by_poll = HashMap::<Uuid, Vec<CardPollOptionResponse>>::new();
    for option in options { options_by_poll.entry(option.poll_id).or_default().push(CardPollOptionResponse { id: option.id, title: option.title, votes: option.votes, voted: option.voted }); }
    Ok(polls.into_iter().map(|poll| CardPollResponse { id: poll.id, question: poll.question, created_by: poll.created_by, created_at: poll.created_at, options: options_by_poll.remove(&poll.id).unwrap_or_default() }).collect())
}

async fn list_card_polls(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<Vec<CardPollResponse>> {
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, current.id).await?;
    Ok(Json(load_card_polls(pool, card_id, current.id).await?))
}

async fn create_card_poll(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateCardPollRequest>) -> ApiResult<CardPollResponse> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let question = valid_text(&request.question, "question", 300)?;
    let mut options = Vec::new();
    let mut option_keys = HashSet::new();
    for option in request.options { let option = option.trim(); if !option.is_empty() { let option = valid_text(option, "option", 120)?.to_owned(); if option_keys.insert(option.to_lowercase()) { options.push(option); } } }
    if !(2..=12).contains(&options.len()) { return Err(ApiError::bad_request("A poll needs from 2 to 12 distinct options.")); }
    let poll_id = Uuid::new_v4();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO card_polls (id, card_id, question, created_by) VALUES ($1, $2, $3, $4)").bind(poll_id).bind(card_id).bind(question).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for (position, title) in options.iter().enumerate() { sqlx::query("INSERT INTO card_poll_options (id, poll_id, title, position) VALUES ($1, $2, $3, $4)").bind(Uuid::new_v4()).bind(poll_id).bind(title).bind(position as i32).execute(&mut *transaction).await.map_err(ApiError::internal)?; }
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card_id, current.id, "Создано голосование", "").await;
    let poll = load_card_polls(pool, card_id, current.id).await?.into_iter().find(|poll| poll.id == poll_id).ok_or_else(|| ApiError::internal(sqlx::Error::RowNotFound))?;
    let _ = state.events.send(());
    Ok(Json(poll))
}

async fn delete_card_poll(State(state): State<AppState>, current: CurrentUser, Path((card_id, poll_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let question = sqlx::query_scalar::<_, String>("DELETE FROM card_polls WHERE id = $1 AND card_id = $2 RETURNING question")
        .bind(poll_id).bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "poll_not_found", "Card poll was not found.".to_owned()))?;
    record_card_activity(pool, card_id, current.id, "Удалено голосование", &question).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn vote_card_poll(State(state): State<AppState>, current: CurrentUser, Path(poll_id): Path<Uuid>, Json(request): Json<VoteCardPollRequest>) -> ApiResult<CardPollResponse> {
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT p.card_id FROM card_polls p INNER JOIN card_poll_options o ON o.poll_id = p.id WHERE p.id = $1 AND o.id = $2")
        .bind(poll_id).bind(request.option_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Poll option was not found."))?;
    ensure_card_access(pool, card_id, current.id).await?;
    ensure_card_unfrozen(pool, card_id).await?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM card_poll_votes WHERE poll_id = $1 AND user_id = $2").bind(poll_id).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    sqlx::query("INSERT INTO card_poll_votes (poll_id, option_id, user_id) VALUES ($1, $2, $3)").bind(poll_id).bind(request.option_id).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    let poll = load_card_polls(pool, card_id, current.id).await?.into_iter().find(|poll| poll.id == poll_id).ok_or_else(|| ApiError::internal(sqlx::Error::RowNotFound))?;
    let _ = state.events.send(());
    Ok(Json(poll))
}

async fn ensure_card_has_no_active_blockers(pool: &PgPool, card_id: Uuid) -> Result<(), ApiError> {
    let blocker = sqlx::query_scalar::<_, String>(
        "SELECT blocker.title FROM card_relations r INNER JOIN cards blocker ON blocker.id = r.source_card_id WHERE r.target_card_id = $1 AND r.relation_type = 'blocks' AND blocker.archived_at IS NULL AND blocker.completed_at IS NULL ORDER BY r.created_at ASC LIMIT 1",
    )
    .bind(card_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    if let Some(title) = blocker {
        return Err(ApiError(StatusCode::CONFLICT, "card_blocked", format!("Cannot complete this card while blocker \"{title}\" is active.")));
    }
    Ok(())
}

// A duplicate is a direct peer relation, not a transitive group: A ↔ B and
// B ↔ C must not let an action on A silently modify C. This keeps the
// operation understandable while still making a paired duplicate behave as
// one task for completion and archiving.
async fn active_duplicate_peer_ids(pool: &PgPool, card_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT DISTINCT CASE WHEN r.source_card_id = $1 THEN r.target_card_id ELSE r.source_card_id END FROM card_relations r INNER JOIN cards peer ON peer.id = CASE WHEN r.source_card_id = $1 THEN r.target_card_id ELSE r.source_card_id END WHERE (r.source_card_id = $1 OR r.target_card_id = $1) AND r.relation_type = 'duplicate' AND peer.archived_at IS NULL",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)
}

async fn active_duplicate_group_ids(pool: &PgPool, card_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    let mut card_ids = active_duplicate_peer_ids(pool, card_id).await?;
    card_ids.push(card_id);
    card_ids.sort_unstable();
    card_ids.dedup();
    Ok(card_ids)
}

async fn update_card_completion(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardCompletionRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    if request.is_completed {
        let review_status = sqlx::query_scalar::<_, String>("SELECT status FROM card_reviews WHERE card_id = $1")
            .bind(card_id)
            .fetch_optional(pool)
            .await
            .map_err(ApiError::internal)?;
        if matches!(review_status.as_deref(), Some("requested" | "changes_requested" | "rejected")) {
            return Err(ApiError::bad_request("Эта задача не может быть завершена, пока review не будет одобрен."));
        }
    }
    let card_ids = active_duplicate_group_ids(pool, card_id).await?;
    for duplicate_id in &card_ids {
        ensure_card_permission(pool, *duplicate_id, current.id, "edit_cards").await?;
        if request.is_completed { ensure_card_has_no_active_blockers(pool, *duplicate_id).await?; }
    }
    let result = sqlx::query("UPDATE cards SET completed_at = CASE WHEN $1 THEN now() ELSE NULL END, updated_at = now() WHERE id = ANY($2) AND archived_at IS NULL")
        .bind(request.is_completed).bind(&card_ids).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    for changed_card_id in card_ids {
        let detail = if changed_card_id == card_id { "" } else { "Синхронизировано со связью «Дубликат»" };
        record_card_activity(pool, changed_card_id, current.id, if request.is_completed { "Задача выполнена" } else { "Задача возвращена в работу" }, detail).await;
        record_discord_card_thread_event(pool, changed_card_id, if request.is_completed { "completed" } else { "reopened" }).await;
    }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn notify_card_waiting_targets(pool: &PgPool, card_id: Uuid, actor_id: Uuid, user_id: Option<Uuid>, role_id: Option<Uuid>, detail: &str) {
    let recipients = if let Some(user_id) = user_id {
        vec![user_id]
    } else if let Some(role_id) = role_id {
        match sqlx::query_scalar::<_, Uuid>(
            "SELECT DISTINCT upr.user_id FROM user_profile_roles upr INNER JOIN cards c ON c.id = $1 INNER JOIN board_members bm ON bm.board_id = c.board_id AND bm.user_id = upr.user_id INNER JOIN users u ON u.id = upr.user_id WHERE upr.role_id = $2 AND u.disabled_at IS NULL",
        )
        .bind(card_id)
        .bind(role_id)
        .fetch_all(pool)
        .await {
            Ok(recipients) => recipients,
            Err(error) => { tracing::error!(?error, card_id = %card_id, "waiting recipient lookup failed"); return; }
        }
    } else { vec![] };
    // A card has one current waiting target. Remove previous pending notices
    // before adding recipients for the current target; otherwise changing a
    // role to a person was silently hidden by the old unread role notice.
    if let Err(error) = sqlx::query("DELETE FROM card_notifications WHERE card_id = $1 AND action = 'Ожидают вашего действия' AND read_at IS NULL")
        .bind(card_id)
        .execute(pool)
        .await
    {
        tracing::error!(?error, card_id = %card_id, "waiting notification cleanup failed");
        return;
    }
    for user_id in recipients.into_iter().filter(|user_id| *user_id != actor_id) {
        let result = sqlx::query(
            "INSERT INTO card_notifications (id, user_id, card_id, actor_id, action, detail) VALUES ($1, $2, $3, $4, 'Ожидают вашего действия', $5)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(card_id)
        .bind(actor_id)
        .bind(detail)
        .execute(pool)
        .await;
        if let Err(error) = result { tracing::error!(?error, card_id = %card_id, user_id = %user_id, "waiting notification insert failed"); }
    }
}

async fn update_card_waiting(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardWaitingRequest>) -> Result<StatusCode, ApiError> {
    if request.user_id.is_some() == request.role_id.is_some() {
        return Err(ApiError::bad_request("Choose exactly one waiting user or role."));
    }
    let note = request.note.trim();
    if note.is_empty() || note.chars().count() > 240 {
        return Err(ApiError::bad_request("Waiting note must contain from 1 to 240 characters."));
    }
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM cards WHERE id = $1 AND archived_at IS NULL")
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    if let Some(user_id) = request.user_id {
        let belongs: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM board_members WHERE board_id = $1 AND user_id = $2)")
            .bind(board_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !belongs { return Err(ApiError::bad_request("Waiting user must belong to this board.")); }
    }
    if let Some(role_id) = request.role_id {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM profile_roles WHERE id = $1)")
            .bind(role_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !exists { return Err(ApiError::bad_request("Waiting role was not found.")); }
    }
    sqlx::query("INSERT INTO card_waiting_for (card_id, user_id, role_id, note, created_by, updated_at) VALUES ($1, $2, $3, $4, $5, now()) ON CONFLICT (card_id) DO UPDATE SET user_id = EXCLUDED.user_id, role_id = EXCLUDED.role_id, note = EXCLUDED.note, created_by = EXCLUDED.created_by, updated_at = now()")
        .bind(card_id).bind(request.user_id).bind(request.role_id).bind(note).bind(current.id)
        .execute(pool).await.map_err(ApiError::internal)?;
    let target = if let Some(user_id) = request.user_id {
        sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = $1").bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?
    } else {
        sqlx::query_scalar::<_, String>("SELECT name FROM profile_roles WHERE id = $1").bind(request.role_id.unwrap()).fetch_one(pool).await.map_err(ApiError::internal)?
    };
    let detail = format!("Ждём {target}: {note}");
    record_card_activity(pool, card_id, current.id, "Ожидается действие", &detail).await;
    notify_card_waiting_targets(pool, card_id, current.id, request.user_id, request.role_id, &detail).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_card_waiting(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let result = sqlx::query("DELETE FROM card_waiting_for WHERE card_id = $1")
        .bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() > 0 {
        record_card_activity(pool, card_id, current.id, "Ожидание снято", "").await;
        let _ = state.events.send(());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn update_card_public_visibility(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardPublicVisibilityRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_full_access(pool, card_id, current.id).await?;
    let updated = sqlx::query("UPDATE cards SET is_public = $1, updated_at = now() WHERE id = $2 AND archived_at IS NULL")
        .bind(request.is_public)
        .bind(card_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()));
    }
    record_card_activity(pool, card_id, current.id, if request.is_public { "Карточка открыта для гостей" } else { "Карточка скрыта от гостей" }, "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_card_access_thresholds(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardAccessThresholdsRequest>) -> Result<StatusCode, ApiError> {
    let valid_view = matches!(request.min_view_preset.as_str(), "viewer" | "contributor" | "editor" | "full_access");
    let valid_edit = matches!(request.min_edit_preset.as_str(), "contributor" | "editor" | "full_access");
    if !valid_view || !valid_edit {
        return Err(ApiError::bad_request("min_view_preset must be viewer, contributor, editor, or full_access; min_edit_preset must be contributor, editor, or full_access."));
    }
    let pool = database(&state)?;
    ensure_card_full_access(pool, card_id, current.id).await?;
    let updated = sqlx::query("UPDATE cards SET min_view_preset = $1, min_edit_preset = $2, updated_at = now() WHERE id = $3 AND archived_at IS NULL")
        .bind(&request.min_view_preset)
        .bind(&request.min_edit_preset)
        .bind(card_id)
        .execute(pool)
        .await
        .map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()));
    }
    record_card_activity(pool, card_id, current.id, "Изменены права карточки", &format!("Просмотр: {}; редактирование: {}", request.min_view_preset, request.min_edit_preset)).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn replace_card_labels(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceCardLabelsRequest>) -> ApiResult<Vec<LabelResponse> > {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    if request.label_ids.len() > 20 { return Err(ApiError::bad_request("A card can have at most 20 labels.")); }
    let label_ids: Vec<Uuid> = request.label_ids.into_iter().collect::<HashSet<_>>().into_iter().collect();
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let board_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT b.id FROM cards c INNER JOIN boards b ON b.id = c.board_id WHERE c.id = $1 AND c.archived_at IS NULL FOR UPDATE",
    )
    .bind(card_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    let matching_labels: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM labels WHERE board_id = $1 AND id = ANY($2)")
        .bind(board_id)
        .bind(&label_ids)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    if matching_labels != label_ids.len() as i64 {
        return Err(ApiError::bad_request("Every label must belong to this board."));
    }
    let previous_labels = sqlx::query_as::<_, (Uuid, String)>("SELECT l.id, l.name FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = $1")
        .bind(card_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM card_labels WHERE card_id = $1")
        .bind(card_id)
        .execute(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    if !label_ids.is_empty() {
        sqlx::query("INSERT INTO card_labels (card_id, label_id) SELECT $1, label_id FROM UNNEST($2::uuid[]) AS selected_labels(label_id) ON CONFLICT DO NOTHING")
            .bind(card_id)
            .bind(&label_ids)
            .execute(&mut *transaction)
            .await
            .map_err(ApiError::internal)?;
    }
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = $1 ORDER BY l.name")
        .bind(card_id)
        .fetch_all(&mut *transaction)
    .await
    .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    let previous_label_ids: HashSet<Uuid> = previous_labels.iter().map(|(id, _)| *id).collect();
    let requested_label_ids: HashSet<Uuid> = label_ids.iter().copied().collect();
    if previous_label_ids != requested_label_ids {
        let added = labels.iter()
            .filter(|label| !previous_label_ids.contains(&label.id))
            .map(|label| format!("+{}", label.name))
            .collect::<Vec<_>>();
        let removed = previous_labels.iter()
            .filter(|(id, _)| !requested_label_ids.contains(id))
            .map(|(_, name)| format!("−{name}"))
            .collect::<Vec<_>>();
        let detail = added.into_iter().chain(removed).collect::<Vec<_>>().join(", ");
        record_card_activity(pool, card_id, current.id, "Обновлены метки карточки", &detail).await;
    }
    let _ = state.events.send(());
    Ok(Json(labels))
}

async fn replace_card_profile_roles(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceCardProfileRolesRequest>) -> ApiResult<Vec<ProfileRoleResponse>> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    if request.role_ids.len() > 20 { return Err(ApiError::bad_request("A card can have at most 20 roles.")); }
    let role_ids: Vec<Uuid> = request.role_ids.into_iter().collect::<HashSet<_>>().into_iter().collect();
    let pool = database(&state)?;
    let card_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND archived_at IS NULL)")
        .bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !card_exists { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    let matching_roles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profile_roles WHERE id = ANY($1)")
        .bind(&role_ids).fetch_one(pool).await.map_err(ApiError::internal)?;
    if matching_roles != role_ids.len() as i64 { return Err(ApiError::bad_request("Every role must exist.")); }
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    sqlx::query("DELETE FROM card_profile_roles WHERE card_id = $1").bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    if !role_ids.is_empty() {
        sqlx::query("INSERT INTO card_profile_roles (card_id, role_id) SELECT $1, role_id FROM UNNEST($2::uuid[]) AS selected_roles(role_id) ON CONFLICT DO NOTHING")
            .bind(card_id).bind(&role_ids).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    let roles = sqlx::query_as::<_, ProfileRoleResponse>("SELECT pr.id, pr.name, pr.color, pr.icon_shape, pr.icon_color FROM card_profile_roles cpr INNER JOIN profile_roles pr ON pr.id = cpr.role_id WHERE cpr.card_id = $1 ORDER BY pr.name")
        .bind(card_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card_id, current.id, "Обновлены роли карточки", "").await;
    let _ = state.events.send(());
    Ok(Json(roles))
}

async fn replace_card_milestone(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceCardMilestoneRequest>) -> ApiResult<Option<MilestoneResponse>> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM cards WHERE id = $1 AND archived_at IS NULL FOR UPDATE")
        .bind(card_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    let milestone = match request.milestone_id {
        Some(milestone_id) => Some(sqlx::query_as::<_, MilestoneResponse>("SELECT id, name, description, color, target_date::text AS target_date FROM milestones WHERE id = $1 AND board_id = $2")
            .bind(milestone_id).bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::bad_request("Milestone must belong to this board."))?),
        None => None,
    };
    let updated = sqlx::query("UPDATE cards SET milestone_id = $1, updated_at = now() WHERE id = $2 AND archived_at IS NULL")
        .bind(request.milestone_id).bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if updated.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(pool, card_id, current.id, if milestone.is_some() { "Назначен milestone" } else { "Milestone снят" }, milestone.as_ref().map(|value| value.name.as_str()).unwrap_or("" )).await;
    let _ = state.events.send(());
    Ok(Json(milestone))
}

async fn replace_card_assignees(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<ReplaceCardAssigneesRequest>) -> ApiResult<Vec<MemberResponse>> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    if request.user_ids.len() > 30 { return Err(ApiError::bad_request("A card can have at most 30 assignees.")); }
    let user_ids: Vec<Uuid> = request.user_ids.into_iter().collect::<HashSet<_>>().into_iter().collect();
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let _workspace_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT b.workspace_id FROM cards c INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $1 AND c.archived_at IS NULL AND m.user_id = $2 FOR UPDATE",
    )
    .bind(card_id)
    .bind(actor_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    let matching_members: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM board_members WHERE board_id = (SELECT board_id FROM cards WHERE id = $1) AND user_id = ANY($2)")
        .bind(card_id)
        .bind(&user_ids)
        .fetch_one(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    if matching_members != user_ids.len() as i64 { return Err(ApiError::bad_request("Every assignee must belong to the card workspace.")); }
    let previous_user_ids = sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM card_assignees WHERE card_id = $1 FOR UPDATE")
        .bind(card_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    let previous_user_id_set: HashSet<Uuid> = previous_user_ids.iter().copied().collect();
    let requested_user_id_set: HashSet<Uuid> = user_ids.iter().copied().collect();
    let actor_was_assignee = previous_user_id_set.contains(&actor_id);
    let actor_is_assignee = user_ids.contains(&actor_id);
    sqlx::query("DELETE FROM card_assignees WHERE card_id = $1")
        .bind(card_id)
        .execute(&mut *transaction)
        .await
        .map_err(ApiError::internal)?;
    if !user_ids.is_empty() {
        sqlx::query("INSERT INTO card_assignees (card_id, user_id) SELECT $1, user_id FROM UNNEST($2::uuid[]) AS selected_users(user_id) ON CONFLICT DO NOTHING")
            .bind(card_id)
            .bind(&user_ids)
            .execute(&mut *transaction)
            .await
            .map_err(ApiError::internal)?;
    }
    let members = sqlx::query_as::<_, MemberResponse>("SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM card_assignees ca INNER JOIN users u ON u.id = ca.user_id WHERE ca.card_id = $1 ORDER BY u.display_name")
        .bind(card_id)
        .fetch_all(&mut *transaction)
    .await
    .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    if !actor_was_assignee && actor_is_assignee {
        record_card_activity(pool, card_id, actor_id, "Присоединился к задаче", "Стал исполнителем").await;
    } else if actor_was_assignee && !actor_is_assignee {
        record_card_activity(pool, card_id, actor_id, "Отказался от задачи", "Перестал быть исполнителем").await;
    } else if previous_user_id_set != requested_user_id_set {
        record_card_activity(pool, card_id, actor_id, "Изменены исполнители", &format!("Исполнителей: {}", members.len())).await;
    }
    let _ = state.events.send(());
    Ok(Json(members))
}

async fn archive_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "delete_cards").await?;
    let card_ids = active_duplicate_group_ids(pool, card_id).await?;
    for duplicate_id in &card_ids { ensure_card_permission(pool, *duplicate_id, current.id, "delete_cards").await?; }
    let actor_id = current.id;
    let result = sqlx::query(
        "UPDATE cards c SET archived_at = now(), updated_at = now() FROM boards b WHERE c.id = ANY($1) AND c.board_id = b.id AND c.archived_at IS NULL AND b.archived_at IS NULL",
    )
    .bind(&card_ids)
    .execute(pool)
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    for changed_card_id in card_ids {
        let detail = if changed_card_id == card_id { "" } else { "Синхронизировано со связью «Дубликат»" };
        record_card_activity(pool, changed_card_id, actor_id, "Задача архивирована", detail).await;
        record_discord_card_thread_event(pool, changed_card_id, "archived").await;
    }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<CardResponse> {
    ensure_archived_card_permission(database(&state)?, card_id, current.id, "delete_cards").await?;
    let card = sqlx::query_as::<_, CardResponse>(
        "UPDATE cards c SET archived_at = NULL, updated_at = now() FROM boards b WHERE c.id = $1 AND c.board_id = b.id AND c.archived_at IS NOT NULL AND b.archived_at IS NULL RETURNING c.id, c.list_id, c.title, c.description, c.start_at::text AS start_at",
    )
    .bind(card_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "archived_card_not_found", "Archived card was not found.".to_owned()))?;
    record_card_activity(database(&state)?, card.id, current.id, "Задача восстановлена", "").await;
    record_discord_card_thread_event(database(&state)?, card.id, "restored").await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn list_archived_cards(State(state): State<AppState>, current: Viewer, Path(board_id): Path<Uuid>, Query(query): Query<ArchivedCardsQuery>) -> ApiResult<ArchivedCardsPageResponse> {
    let page_size = query.limit.unwrap_or(50).clamp(1, 100);
    let mut cards = sqlx::query_as::<_, ArchivedCardRow>(
        "SELECT c.id, c.list_id, c.title, c.description, c.priority, c.completed_at::text AS completed_at, c.archived_at::text AS archived_at FROM cards c INNER JOIN lists l ON l.id = c.list_id INNER JOIN boards b ON b.id = c.board_id WHERE c.board_id = $1 AND c.archived_at IS NOT NULL AND ($2::uuid IS NOT NULL OR l.is_public) AND ((b.visibility = 'public' AND c.is_public) OR ($2::uuid IS NOT NULL AND (EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access')) OR EXISTS (SELECT 1 FROM board_members bm INNER JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id WHERE bm.board_id = b.id AND bm.user_id = $2 AND CASE wm.role::text WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 WHEN 'owner' THEN 4 ELSE -1 END >= CASE c.min_view_preset WHEN 'viewer' THEN 0 WHEN 'contributor' THEN 1 WHEN 'editor' THEN 2 WHEN 'full_access' THEN 3 ELSE 0 END)))) AND ($3::uuid IS NULL OR (c.archived_at, c.id) < (SELECT cursor.archived_at, cursor.id FROM cards cursor WHERE cursor.id = $3 AND cursor.board_id = $1 AND cursor.archived_at IS NOT NULL)) ORDER BY c.archived_at DESC, c.id DESC LIMIT $4",
    )
    .bind(board_id)
    .bind(current.0.map(|user| user.id))
    .bind(query.cursor)
    .bind(page_size + 1)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    let has_more = cards.len() > page_size as usize;
    cards.truncate(page_size as usize);
    let next_cursor = has_more.then(|| cards.last().map(|card| card.id)).flatten();
    let card_ids: Vec<Uuid> = cards.iter().map(|card| card.id).collect();
    let card_labels = sqlx::query_as::<_, CardLabelRow>("SELECT cl.card_id, l.id, l.name, l.color, l.icon_shape, l.icon_color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = ANY($1) ORDER BY l.name")
        .bind(&card_ids).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    let card_roles = sqlx::query_as::<_, CardProfileRoleRow>("SELECT cpr.card_id, pr.id, pr.name, pr.color, pr.icon_shape, pr.icon_color FROM card_profile_roles cpr INNER JOIN profile_roles pr ON pr.id = cpr.role_id WHERE cpr.card_id = ANY($1) ORDER BY pr.name")
        .bind(&card_ids).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    let mut card_assignees = sqlx::query_as::<_, CardAssigneeRow>("SELECT ca.card_id, u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM card_assignees ca INNER JOIN users u ON u.id = ca.user_id WHERE ca.card_id = ANY($1) ORDER BY u.display_name")
        .bind(&card_ids).fetch_all(database(&state)?).await.map_err(ApiError::internal)?;
    if current.0.is_none() {
        for assignee in &mut card_assignees {
            if assignee.avatar_url.is_some() { assignee.avatar_url = Some(format!("/v1/public/boards/{board_id}/avatars/{}", assignee.id)); }
        }
    }
    let items = cards.into_iter().map(|card| ArchivedCardResponse {
        id: card.id,
        list_id: card.list_id,
        title: card.title,
        description: card.description,
        priority: card.priority,
        completed_at: card.completed_at,
        archived_at: card.archived_at,
        labels: card_labels.iter().filter(|label| label.card_id == card.id).map(|label| LabelResponse { id: label.id, name: label.name.clone(), color: label.color.clone(), icon_shape: label.icon_shape.clone(), icon_color: label.icon_color.clone() }).collect(),
        roles: card_roles.iter().filter(|role| role.card_id == card.id).map(|role| ProfileRoleResponse { id: role.id, name: role.name.clone(), color: role.color.clone(), icon_shape: role.icon_shape.clone(), icon_color: role.icon_color.clone() }).collect(),
        assignees: card_assignees.iter().filter(|member| member.card_id == card.id).map(|member| MemberResponse { id: member.id, display_name: member.display_name.clone(), avatar_url: member.avatar_url.clone() }).collect(),
    }).collect();
    Ok(Json(ArchivedCardsPageResponse { items, next_cursor }))
}

async fn move_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<MoveCardRequest>) -> ApiResult<CardResponse> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let pool = database(&state)?;
    let target_board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM lists WHERE id = $1")
        .bind(request.target_list_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "Target column was not found.".to_owned()))?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let card = sqlx::query_as::<_, CardResponse>(
        "WITH source AS (SELECT c.id, c.board_id FROM cards c INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $1 AND c.archived_at IS NULL AND m.user_id = $4 FOR UPDATE), target AS (SELECT id, board_id FROM lists WHERE id = $2 FOR UPDATE), anchor AS (SELECT c.position FROM cards c, target, source WHERE c.id = $3 AND c.list_id = target.id AND c.id <> source.id FOR UPDATE), previous AS (SELECT c.position FROM cards c, target, source WHERE c.list_id = target.id AND c.id <> source.id AND c.position < (SELECT position FROM anchor) ORDER BY c.position DESC LIMIT 1) UPDATE cards c SET list_id = target.id, position = CASE WHEN $3 IS NULL THEN (SELECT COALESCE(MAX(position), 0) + 1000 FROM cards WHERE list_id = target.id AND id <> c.id) WHEN (SELECT position FROM previous) IS NULL THEN (SELECT position - 1000 FROM anchor) ELSE ((SELECT position FROM previous) + (SELECT position FROM anchor)) / 2 END, updated_at = now() FROM source, target WHERE c.id = source.id AND source.board_id = target.board_id AND ($3 IS NULL OR EXISTS (SELECT 1 FROM anchor)) RETURNING c.id, c.list_id, c.title, c.description, c.start_at::text AS start_at",
    )
    .bind(card_id)
    .bind(request.target_list_id)
    .bind(request.before_card_id)
    .bind(actor_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_or_list_not_found", "Card or target list was not found.".to_owned()))?;
    sqlx::query("DELETE FROM board_freeform_card_positions WHERE card_id = $1")
        .bind(card_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card.id, actor_id, "Перемещена задача", "Изменена колонка или порядок").await;
    run_card_move_automations(pool, target_board_id, card.id, request.target_list_id, actor_id).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[tokio::test]
    async fn rate_limiter_scopes_attempts_to_source_and_reclaims_expired_buckets() {
        let limiter = RateLimiter::with_limits(Duration::from_secs(60), 2, 2);
        let started = Instant::now();
        let first = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        let second = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2));
        let third = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 3));

        assert!(limiter.check_at("login", first, started).await.is_ok());
        assert!(limiter.check_at("login", first, started).await.is_ok());
        assert!(limiter.check_at("login", first, started).await.is_err());
        assert!(limiter.check_at("login", second, started).await.is_ok());
        assert!(limiter.check_at("login", third, started).await.is_err());
        assert!(limiter.check_at("login", third, started + Duration::from_secs(61)).await.is_ok());
    }

    #[test]
    fn request_source_ip_only_uses_forwarded_header_for_trusted_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.7"));
        let peer = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080));

        assert_eq!(request_source_ip(&headers, peer, false), peer.ip());
        assert_eq!(request_source_ip(&headers, peer, true), IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)));
    }

    #[test]
    fn comment_push_avatar_urls_become_public_absolute_urls() {
        let base = reqwest::Url::parse("https://flowboard.zei.su").unwrap();
        assert_eq!(
            absolute_flowboard_url(Some(&base), "/v1/avatars/00000000-0000-0000-0000-000000000000"),
            Some("https://flowboard.zei.su/v1/avatars/00000000-0000-0000-0000-000000000000".to_owned()),
        );
        assert_eq!(
            absolute_flowboard_url(Some(&base), "https://cdn.example.test/avatar.webp"),
            Some("https://cdn.example.test/avatar.webp".to_owned()),
        );
    }

    #[test]
    fn discord_outbound_comment_body_replaces_internal_default_stickers() {
        assert_eq!(discord_outbound_comment_body("Готово [[sticker:✅]]"), "Готово ✅");
        assert_eq!(discord_outbound_comment_body("[[sticker:🔥]] и [[sticker:💯]]"), "🔥 и 💯");
        assert_eq!(discord_outbound_comment_body("[[sticker:\n]]"), "[[sticker:\n]]");
    }

    #[test]
    fn discord_outbound_comments_remove_voice_markdown_only() {
        let voice_id = Uuid::nil();
        let voice = format!("![audio:voice.webm](/v1/attachments/{voice_id}/content)");
        let image = "![image.png](/v1/attachments/11111111-1111-1111-1111-111111111111/content)";

        assert_eq!(
            remove_audio_attachment_markdown(&format!("До {voice} после {image}"), voice_id),
            format!("До  после {image}"),
        );
    }

}
