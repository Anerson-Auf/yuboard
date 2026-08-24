use std::{collections::{HashMap, HashSet, VecDeque}, convert::Infallible, env, path::PathBuf, sync::Arc, time::{Duration, Instant}};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{DefaultBodyLimit, FromRequestParts, Multipart, Path, Query, State},
    http::{header, request::Parts, HeaderName, HeaderValue, Method, StatusCode},
    response::{sse::{Event, Sse}, IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{DateTime, Datelike, FixedOffset, Utc};
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
    events: broadcast::Sender<()>,
    auth_rate_limiter: RateLimiter,
}

#[derive(Clone)]
struct RateLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
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
}

#[derive(Serialize)]
struct BoardBackgroundUploadResponse {
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
    visibility: String,
}

#[derive(Deserialize)]
struct CreateListRequest {
    title: String,
}

#[derive(Deserialize)]
struct MoveListRequest {
    before_list_id: Option<Uuid>,
}

#[derive(Serialize, FromRow)]
#[derive(Clone)]
struct ListResponse {
    id: Uuid,
    title: String,
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
}

#[derive(Deserialize)]
struct UpdateListRequest {
    title: String,
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
struct UpdateChecklistItemRequest {
    is_completed: bool,
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
}

#[derive(Deserialize)]
struct DiscordAttachmentRequest {
    url: String,
    filename: String,
    media_type: String,
    #[serde(default)]
    byte_size: i64,
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

#[derive(Deserialize)]
struct UpdateCardCompletionRequest {
    is_completed: bool,
}

#[derive(Deserialize)]
struct CreateLabelRequest {
    name: String,
    color: String,
}

#[derive(Deserialize)]
struct ReplaceCardLabelsRequest {
    label_ids: Vec<Uuid>,
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

#[derive(Clone, Serialize, FromRow)]
struct CardResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
}

#[derive(Serialize, FromRow)]
struct DiscordCardStatusResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    is_completed: bool,
    completed_at: Option<String>,
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

#[derive(Clone, Serialize, FromRow)]
struct ArchivedCardResponse {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    archived_at: String,
}

#[derive(Clone, FromRow)]
struct BoardCardRow {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    background_image_url: Option<String>,
    due_at: Option<String>,
    cover_attachment_id: Option<Uuid>,
    cover_url: Option<String>,
    cover_mode: String,
    completed_at: Option<String>,
    checklist_total: i64,
    checklist_completed: i64,
    comment_count: i64,
    attachment_count: i64,
}

#[derive(Clone, Serialize, FromRow)]
struct LabelResponse {
    id: Uuid,
    name: String,
    color: String,
}

#[derive(FromRow)]
struct CardLabelRow {
    card_id: Uuid,
    id: Uuid,
    name: String,
    color: String,
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

#[derive(Clone, Serialize)]
struct BoardCard {
    id: Uuid,
    list_id: Uuid,
    title: String,
    description: String,
    background_image_url: Option<String>,
    due_at: Option<String>,
    cover_attachment_id: Option<Uuid>,
    cover_url: Option<String>,
    cover_mode: String,
    completed_at: Option<String>,
    checklist_total: i64,
    checklist_completed: i64,
    comment_count: i64,
    attachment_count: i64,
    labels: Vec<LabelResponse>,
    assignees: Vec<MemberResponse>,
}

#[derive(Clone, Serialize, FromRow)]
struct ChecklistItemResponse {
    id: Uuid,
    title: String,
    is_completed: bool,
}

#[derive(Clone, Serialize)]
struct ChecklistResponse {
    id: Uuid,
    title: String,
    items: Vec<ChecklistItemResponse>,
}

#[derive(FromRow)]
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
    checklist_title: String,
}

#[derive(Clone, Serialize)]
struct CommentResponse {
    id: Uuid,
    body: String,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
    parent_comment_id: Option<Uuid>,
    created_at: String,
    edited_at: Option<String>,
    reactions: Vec<CommentReactionResponse>,
}

#[derive(FromRow)]
struct CommentRow {
    id: Uuid,
    body: String,
    author_id: Option<Uuid>,
    author_name: String,
    author_avatar_url: Option<String>,
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
    actor_name: Option<String>,
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
}

#[derive(Serialize)]
struct BoardDetail {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    background_image_url: Option<String>,
    visibility: String,
    labels: Vec<LabelResponse>,
    members: Vec<MemberResponse>,
    lists: Vec<BoardList>,
}

#[derive(Serialize)]
struct BoardList {
    id: Uuid,
    title: String,
    cards: Vec<BoardCard>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let upload_dir = PathBuf::from(env::var("FLOWBOARD_UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_owned()));
    tokio::fs::create_dir_all(&upload_dir).await.expect("could not create FLOWBOARD_UPLOAD_DIR");

    let cookie_secure = env::var("FLOWBOARD_COOKIE_SECURE").map(|value| value != "false").unwrap_or(false);
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

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/auth/register", post(register))
        .route("/v1/auth/login", post(login))
        .route("/v1/auth/accept-invitation", post(accept_account_invitation))
        .route("/v1/auth/logout", post(logout))
        .route("/v1/auth/setup", get(auth_setup))
        .route("/v1/auth/me", get(current_account).patch(update_profile))
        .route("/v1/auth/password", post(change_password))
        .route("/v1/auth/sessions", get(list_sessions).delete(revoke_other_sessions))
        .route("/v1/auth/sessions/{session_id}", axum::routing::delete(revoke_session))
        .route("/v1/auth/avatar", get(download_avatar).post(upload_avatar))
        .route("/v1/avatars/{user_id}", get(download_user_avatar))
        .route("/v1/public/boards/{board_id}/background", get(download_public_board_background))
        .route("/v1/public/boards/{board_id}/avatars/{user_id}", get(download_public_board_avatar))
        .route("/v1/workspaces", get(list_workspaces).post(create_workspace))
        .route("/v1/workspaces/{workspace_id}", axum::routing::delete(delete_workspace))
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
        .route("/v1/boards/{board_id}/background", put(update_board_background))
        .route("/v1/boards/{board_id}/background/file", get(download_board_background).post(upload_board_background))
        .route("/v1/boards/{board_id}/visibility", put(update_board_visibility))
        .route("/v1/boards/{board_id}/integrations/discord", get(list_discord_integrations).post(create_discord_integration))
        .route("/v1/boards/{board_id}/integrations/discord/{integration_id}", axum::routing::delete(revoke_discord_integration))
        .route("/v1/boards/{board_id}/export", get(export_board))
        .route("/v1/boards/{board_id}/archived-cards", get(list_archived_cards))
        .route("/v1/boards/{board_id}/events", get(board_events))
        .route("/v1/boards/{board_id}/labels", post(create_label))
        .route("/v1/boards/{board_id}/lists", post(create_list))
        .route("/v1/lists/{list_id}", patch(update_list).delete(delete_list))
        .route("/v1/lists/{list_id}/move", post(move_list))
        .route("/v1/lists/{list_id}/cards", post(create_card))
        .route("/v1/cards/{card_id}/move", post(move_card))
        .route("/v1/cards/{card_id}/restore", post(restore_card))
        .route("/v1/cards/{card_id}", axum::routing::patch(update_card).delete(archive_card))
        .route("/v1/cards/{card_id}/due-date", patch(update_due_date).delete(clear_due_date))
        .route("/v1/cards/{card_id}/labels", put(replace_card_labels))
        .route("/v1/cards/{card_id}/assignees", put(replace_card_assignees))
        .route("/v1/cards/{card_id}/cover", put(update_card_cover))
        .route("/v1/cards/{card_id}/background", put(update_card_background))
        .route("/v1/cards/{card_id}/background/file", get(download_card_background).post(upload_card_background))
        .route("/v1/cards/{card_id}/completion", patch(update_card_completion))
        .route("/v1/cards/{card_id}/details", get(get_card_detail))
        .route("/v1/cards/{card_id}/diagram", get(get_card_diagram).put(replace_card_diagram))
        .route("/v1/cards/{card_id}/checklists", post(create_checklist))
        .route("/v1/checklists/{checklist_id}", axum::routing::delete(delete_checklist))
        .route("/v1/checklists/{checklist_id}/items", post(create_checklist_item))
        .route("/v1/checklist-items/{item_id}", patch(update_checklist_item).delete(delete_checklist_item))
        .route("/v1/cards/{card_id}/comments", post(create_comment))
        .route("/v1/integrations/discord/lists", get(list_discord_board_lists))
        .route("/v1/integrations/discord/cards", get(list_discord_board_cards).post(create_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}", get(get_discord_card).delete(archive_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}/move", post(move_discord_card))
        .route("/v1/integrations/discord/cards/{card_id}/cover", post(set_discord_card_cover))
        .route("/v1/integrations/discord/cards/{card_id}/completion", patch(set_discord_card_completion))
        .route("/v1/integrations/discord/cards/{card_id}/comments", get(list_discord_card_comments).post(create_discord_comment))
        .route("/v1/comments/{comment_id}", patch(update_comment).delete(delete_comment))
        .route("/v1/comments/{comment_id}/reactions", post(toggle_comment_reaction))
        .route("/v1/cards/{card_id}/attachments", post(upload_attachment))
        .route("/v1/attachments/{attachment_id}", get(download_attachment).delete(delete_attachment))
        .route("/v1/attachments/{attachment_id}/content", get(download_attachment))
        .with_state(AppState { database, upload_dir, cookie_secure, events, auth_rate_limiter: RateLimiter { attempts: Arc::new(Mutex::new(HashMap::new())) } })
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let bind_address = env::var("FLOWBOARD_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("FLOWBOARD_BIND_ADDR must be available");
    println!("Flowboard API is listening on http://{bind_address}");
    axum::serve(listener, app).await.expect("API server failed");
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
    async fn check(&self, action: &str, subject: &str) -> Result<(), ApiError> {
        const WINDOW: Duration = Duration::from_secs(10 * 60);
        const LIMIT: usize = 10;
        let now = Instant::now();
        let mut attempts = self.attempts.lock().await;
        let entries = attempts.entry(format!("{action}:{subject}")).or_default();
        while entries.front().is_some_and(|time| now.duration_since(*time) > WINDOW) { entries.pop_front(); }
        if entries.len() >= LIMIT { return Err(ApiError::too_many_requests("Too many attempts. Try again in a few minutes.")); }
        entries.push_back(now);
        Ok(())
    }
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
        _ => None,
    };
    from_media_type.or_else(|| match original_name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("jpg"), "png" => Some("png"), "gif" => Some("gif"), "webp" => Some("webp"), "mp4" => Some("mp4"), "webm" => Some("webm"), "mov" => Some("mov"), _ => None,
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
    jar: CookieJar,
    Json(request): Json<RegisterRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    let username = valid_username(&request.username)?;
    let password = valid_password(&request.password)?;
    state.auth_rate_limiter.check("register", &username).await?;
    let pool = database(&state)?;
    if has_registered_account(pool).await? {
        return Err(ApiError::forbidden("Registration is invite-only after the first workspace owner is created."));
    }
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| ApiError::internal(sqlx::Error::Protocol("password hash failed".into())))?
        .to_string();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
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
            "UPDATE users SET username = $1, display_name = $1, password_hash = $2, disabled_at = NULL WHERE id = $3 RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner",
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
    jar: CookieJar,
    Json(request): Json<LoginRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    let username = valid_username(&request.username)?;
    state.auth_rate_limiter.check("login", &username).await?;
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
    jar: CookieJar,
    Json(request): Json<AcceptInvitationRequest>,
) -> Result<(CookieJar, Json<AuthResponse>), ApiError> {
    if request.token.len() < 32 || request.token.len() > 200 {
        return Err(ApiError::bad_request("Invitation token is invalid."));
    }
    state.auth_rate_limiter.check("invite", &request.token).await?;
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

async fn update_profile(State(state): State<AppState>, current: CurrentUser, Json(request): Json<UpdateProfileRequest>) -> ApiResult<AuthResponse> {
    let username = valid_username(&request.username)?;
    let account = sqlx::query_as::<_, PasswordAccount>("UPDATE users SET username = $1, display_name = $1 WHERE id = $2 AND disabled_at IS NULL RETURNING id, username, display_name, password_hash, disabled_at, avatar_key, avatar_media_type, is_system_owner")
        .bind(username).bind(current.id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(auth_response(account)))
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

async fn create_account_invitation(State(state): State<AppState>, current: CurrentUser) -> ApiResult<AccountInvitationResponse> {
    let pool = database(&state)?;
    ensure_system_owner(pool, current.id).await?;
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
    let keys = sqlx::query_scalar::<_, String>("SELECT a.object_key FROM attachments a JOIN cards c ON c.id = a.card_id JOIN boards b ON b.id = c.board_id WHERE b.workspace_id = $1 AND a.object_key IS NOT NULL UNION ALL SELECT cb.object_key FROM card_backgrounds cb JOIN cards c ON c.id = cb.card_id JOIN boards b ON b.id = c.board_id WHERE b.workspace_id = $1")
        .bind(workspace_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let deleted = sqlx::query("DELETE FROM workspaces WHERE id = $1").bind(workspace_id).execute(pool).await.map_err(ApiError::internal)?;
    if deleted.rows_affected() == 0 { return Err(ApiError::bad_request("Workspace is unavailable.")); }
    for key in keys { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    record_audit(pool, current.id, None, None, "workspace.deleted").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_workspaces(State(state): State<AppState>, current: CurrentUser) -> ApiResult<Vec<WorkspaceResponse>> {
    let actor_id = current.id;
    let rows = sqlx::query_as::<_, WorkspaceResponse>(
        "SELECT w.id, w.name FROM workspaces w WHERE w.archived_at IS NULL AND (EXISTS (SELECT 1 FROM users u WHERE u.id = $1 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members m WHERE m.workspace_id = w.id AND m.user_id = $1) OR EXISTS (SELECT 1 FROM boards b JOIN board_members bm ON bm.board_id = b.id WHERE b.workspace_id = w.id AND b.archived_at IS NULL AND bm.user_id = $1)) ORDER BY w.created_at DESC",
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

async fn ensure_board_permission(pool: &PgPool, board_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))")
        .bind(board_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
}

async fn ensure_card_permission(pool: &PgPool, card_id: Uuid, actor_id: Uuid, permission: &str) -> Result<(), ApiError> {
    let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards c JOIN boards b ON b.id = c.board_id LEFT JOIN board_members bm ON bm.board_id = b.id AND bm.user_id = $2 WHERE c.id = $1 AND c.archived_at IS NULL AND b.archived_at IS NULL AND flowboard_has_permission(b.workspace_id, $2, $3::workspace_permission) AND (bm.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))))")
        .bind(card_id).bind(actor_id).bind(permission).fetch_optional(pool).await.map_err(ApiError::internal)?.unwrap_or(false);
    if allowed { Ok(()) } else { Err(ApiError::forbidden("This action is not permitted in the workspace.")) }
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
    let member = sqlx::query_as::<_, WorkspaceMemberManagementResponse>(
        "UPDATE workspace_members wm SET role = $1::workspace_role FROM users u WHERE wm.workspace_id = $2 AND wm.user_id = $3 AND wm.role <> 'owner' AND u.id = wm.user_id RETURNING u.id, u.username, wm.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url",
    )
    .bind(preset)
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::bad_request("Workspace owner role cannot be changed."))?;
    record_audit(pool, current.id, Some(workspace_id), Some(user_id), "workspace_member.preset_changed").await;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn remove_workspace_member(State(state): State<AppState>, current: CurrentUser, Path((workspace_id, user_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_workspace_owner(pool, workspace_id, current.id).await?;
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
    let member = sqlx::query_as::<_, WorkspaceMemberManagementResponse>("WITH target AS (SELECT b.workspace_id FROM boards b WHERE b.id = $2), updated AS (UPDATE workspace_members wm SET role = $1::workspace_role FROM target WHERE wm.workspace_id = target.workspace_id AND wm.user_id = $3 AND wm.role <> 'owner' AND EXISTS (SELECT 1 FROM board_members bm WHERE bm.board_id = $2 AND bm.user_id = wm.user_id) RETURNING wm.user_id, wm.role) UPDATE board_members bm SET role = CASE WHEN $1 = 'viewer' THEN 'viewer'::board_role ELSE 'editor'::board_role END FROM updated, users u WHERE bm.board_id = $2 AND bm.user_id = updated.user_id AND u.id = updated.user_id RETURNING u.id, u.username, updated.role::text AS preset, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url")
        .bind(preset).bind(board_id).bind(user_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Project owner role cannot be changed."))?;
    record_audit(pool, current.id, None, Some(user_id), "project_member.preset_changed").await;
    let _ = state.events.send(());
    Ok(Json(member))
}

async fn remove_board_member(State(state): State<AppState>, current: CurrentUser, Path((board_id, user_id)): Path<(Uuid, Uuid)>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "remove_members").await?;
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
    let permissions: Vec<String> = request.permissions.into_iter().collect::<HashSet<_>>().into_iter().collect();
    if permissions.len() > WORKSPACE_PERMISSIONS.len() || permissions.iter().any(|permission| !WORKSPACE_PERMISSIONS.contains(&permission.as_str())) {
        return Err(ApiError::bad_request("Unknown workspace permission."));
    }
    let member_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members WHERE workspace_id = $1 AND user_id = $2 AND role <> 'owner')")
        .bind(workspace_id).bind(user_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !member_exists { return Err(ApiError::bad_request("Permissions for workspace owner cannot be edited.")); }
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
        "INSERT INTO workspaces (id, name, created_by) VALUES ($1, $2, $3) RETURNING id, name",
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
        "SELECT b.id, b.workspace_id, b.title, CASE WHEN b.background_image_url = '/v1/boards/' || b.id::text || '/background/file' THEN b.background_image_url || '?v=' || (floor(EXTRACT(EPOCH FROM b.updated_at) * 1000)::bigint)::text ELSE b.background_image_url END AS background_image_url, b.visibility::text AS visibility FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public')",
    )
    .bind(board_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let lists = sqlx::query_as::<_, ListResponse>("SELECT id, title FROM lists WHERE board_id = $1 ORDER BY position")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let cards = sqlx::query_as::<_, BoardCardRow>("SELECT c.id, c.list_id, c.title, c.description, c.background_image_url, c.due_at::text AS due_at, c.cover_attachment_id, CASE WHEN a.id IS NULL THEN NULL ELSE COALESCE(a.external_url, '/v1/attachments/' || a.id::text || '/content') END AS cover_url, c.cover_mode, c.completed_at::text AS completed_at, (SELECT COUNT(*) FROM checklist_items ci WHERE ci.card_id = c.id) AS checklist_total, (SELECT COUNT(*) FROM checklist_items ci WHERE ci.card_id = c.id AND ci.is_completed) AS checklist_completed, (SELECT COUNT(*) FROM comments cm WHERE cm.card_id = c.id) AS comment_count, (SELECT COUNT(*) FROM attachments at WHERE at.card_id = c.id) AS attachment_count FROM cards c LEFT JOIN attachments a ON a.id = c.cover_attachment_id WHERE c.board_id = $1 AND c.archived_at IS NULL ORDER BY c.position")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let card_ids: Vec<Uuid> = cards.iter().map(|card| card.id).collect();
    let card_labels = sqlx::query_as::<_, CardLabelRow>("SELECT cl.card_id, l.id, l.name, l.color FROM card_labels cl INNER JOIN cards c ON c.id = cl.card_id INNER JOIN labels l ON l.id = cl.label_id AND l.board_id = c.board_id WHERE cl.card_id = ANY($1) ORDER BY l.name")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let mut card_assignees = sqlx::query_as::<_, CardAssigneeRow>("SELECT ca.card_id, u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM card_assignees ca INNER JOIN users u ON u.id = ca.user_id WHERE ca.card_id = ANY($1) ORDER BY u.display_name")
        .bind(&card_ids)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    if actor_id.is_none() {
        for assignee in &mut card_assignees {
            if assignee.avatar_url.is_some() { assignee.avatar_url = Some(format!("/v1/public/boards/{}/avatars/{}", board.id, assignee.id)); }
        }
    }
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT id, name, color FROM labels WHERE board_id = $1 ORDER BY name")
        .bind(board_id)
        .fetch_all(pool)
        .await
        .map_err(ApiError::internal)?;
    let cards: Vec<BoardCard> = cards.into_iter().map(|card| BoardCard {
        id: card.id,
        list_id: card.list_id,
        title: card.title,
        description: card.description,
        background_image_url: card.background_image_url,
        due_at: card.due_at,
        cover_attachment_id: card.cover_attachment_id,
        cover_url: card.cover_url,
        cover_mode: card.cover_mode,
        completed_at: card.completed_at,
        checklist_total: card.checklist_total,
        checklist_completed: card.checklist_completed,
        comment_count: card.comment_count,
        attachment_count: card.attachment_count,
        labels: card_labels.iter().filter(|label| label.card_id == card.id).map(|label| LabelResponse { id: label.id, name: label.name.clone(), color: label.color.clone() }).collect(),
        assignees: card_assignees.iter().filter(|member| member.card_id == card.id).map(|member| MemberResponse { id: member.id, display_name: member.display_name.clone(), avatar_url: member.avatar_url.clone() }).collect(),
    }).collect();
    let mut members = sqlx::query_as::<_, MemberResponse>("SELECT u.id, u.display_name, CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END AS avatar_url FROM board_members bm INNER JOIN users u ON u.id = bm.user_id WHERE bm.board_id = $1 ORDER BY u.display_name")
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
        cards: cards.iter().filter(|card| card.list_id == list.id).cloned().collect(),
    }).collect();
    let uploaded_background_url = format!("/v1/boards/{}/background/file", board.id);
    let background_image_url = if actor_id.is_none() && board.background_image_url.as_deref().is_some_and(|url| url.starts_with(&uploaded_background_url)) {
        Some(format!("/v1/public/boards/{}/background", board.id))
    } else { board.background_image_url };
    Ok(Json(BoardDetail { id: board.id, workspace_id: board.workspace_id, title: board.title, background_image_url, visibility: board.visibility, labels, members, lists }))
}

async fn board_events(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let can_view: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM boards b LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE b.id = $1 AND b.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public'))",
    )
    .bind(board_id)
    .bind(current.id)
    .fetch_one(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if !can_view { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let pool = database(&state)?.clone();
    let actor_id = current.id;
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
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
    let keys = sqlx::query_scalar::<_, String>("SELECT a.object_key FROM attachments a JOIN cards c ON c.id = a.card_id WHERE c.board_id = $1 AND a.object_key IS NOT NULL UNION ALL SELECT cb.object_key FROM card_backgrounds cb JOIN cards c ON c.id = cb.card_id WHERE c.board_id = $1")
        .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    let result = sqlx::query("DELETE FROM boards WHERE id = $1 AND archived_at IS NULL")
        .bind(board_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    for key in keys { let _ = tokio::fs::remove_file(state.upload_dir.join(key)).await; }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn update_board_background(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardBackgroundRequest>) -> Result<StatusCode, ApiError> {
    ensure_board_permission(database(&state)?, board_id, current.id, "manage_permissions").await?;
    let url = match request.background_image_url {
        Some(value) if !value.trim().is_empty() => {
            let value = value.trim();
            if value.len() > 2_000 || !(value.starts_with("https://") || value.starts_with("/v1/")) { return Err(ApiError::bad_request("Background must be an HTTPS image URL or an uploaded Flowboard file.")); }
            Some(value.to_owned())
        }
        _ => None,
    };
    let result = sqlx::query("UPDATE boards b SET background_image_url = $1, updated_at = now() FROM board_members m WHERE b.id = $2 AND m.board_id = b.id AND m.user_id = $3 AND b.archived_at IS NULL")
        .bind(url).bind(board_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_board_background(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, mut multipart: Multipart) -> ApiResult<BoardBackgroundUploadResponse> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
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

async fn update_board_visibility(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<UpdateBoardVisibilityRequest>) -> Result<StatusCode, ApiError> {
    ensure_board_permission(database(&state)?, board_id, current.id, "manage_permissions").await?;
    let visibility = match request.visibility.as_str() { "public" => "public", "private" => "private", _ => return Err(ApiError::bad_request("Visibility must be public or private.")) };
    let result = sqlx::query("UPDATE boards b SET visibility = $1::board_visibility, updated_at = now() FROM board_members m WHERE b.id = $2 AND m.board_id = b.id AND m.user_id = $3 AND b.archived_at IS NULL")
        .bind(visibility).bind(board_id).bind(current.id).execute(database(&state)?).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned())); }
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn list_discord_integrations(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<DiscordIntegrationResponse>> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
    let integrations = sqlx::query_as::<_, DiscordIntegrationResponse>(
        "SELECT id, name, default_list_id, created_at::text AS created_at, last_used_at::text AS last_used_at, NULL::text AS token FROM discord_integrations WHERE board_id = $1 AND revoked_at IS NULL ORDER BY created_at DESC",
    )
    .bind(board_id).fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(integrations))
}

async fn create_discord_integration(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateDiscordIntegrationRequest>) -> ApiResult<DiscordIntegrationResponse> {
    let pool = database(&state)?;
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
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
    ensure_board_permission(pool, board_id, current.id, "manage_permissions").await?;
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
    ensure_workspace_owner(database(&state)?, workspace_id, current.id).await?;
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
        sqlx::query("INSERT INTO lists (id, board_id, title, position) VALUES ($1, $2, $3, $4)")
            .bind(list_id).bind(board_id).bind(import_string(list, "name", 200).unwrap_or_else(|| "Без названия".to_owned())).bind(((index + 1) * 1000) as i64).execute(&mut *transaction).await.map_err(ApiError::internal)?;
        list_ids.insert(source_id, list_id); imported_lists += 1;
    }
    if list_ids.is_empty() { return Err(ApiError::bad_request("Import contains no active lists.")); }
    let mut card_ids = HashMap::new(); let mut card_positions: HashMap<Uuid, i64> = HashMap::new(); let mut imported_cards = 0;
    for card in source_cards.iter().filter(|card| !card.get("closed").and_then(Value::as_bool).unwrap_or(false)) {
        let (Some(source_id), Some(list_source_id)) = (import_string(card, "id", 128), import_string(card, "idList", 128)) else { continue; };
        let Some(&list_id) = list_ids.get(&list_source_id) else { continue; };
        let position = card_positions.entry(list_id).and_modify(|value| *value += 1000).or_insert(1000);
        let card_id = Uuid::new_v4();
        sqlx::query("INSERT INTO cards (id, board_id, list_id, title, description, position, due_at, completed_at, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(card_id).bind(board_id).bind(list_id).bind(import_string(card, "name", 500).unwrap_or_else(|| "Без названия".to_owned())).bind(import_string(card, "desc", 20_000).unwrap_or_default()).bind(*position).bind(import_timestamp(card.get("due"))).bind(import_timestamp(card.get("dateCompleted"))).bind(current.id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
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
    ensure_board_permission(database(&state)?, board_id, current.id, "manage_permissions").await?; let pool = database(&state)?;
    let board = sqlx::query_as::<_, BoardAccess>("SELECT id, workspace_id, title, background_image_url, visibility::text AS visibility FROM boards WHERE id = $1 AND archived_at IS NULL").bind(board_id).fetch_optional(pool).await.map_err(ApiError::internal)?.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let lists = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id::text, 'name', title, 'closed', false, 'pos', position) ORDER BY position), '[]'::jsonb) FROM lists WHERE board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let labels = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', id::text, 'name', name, 'color', color) ORDER BY name), '[]'::jsonb) FROM labels WHERE board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let cards = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', c.id::text, 'name', c.title, 'desc', c.description, 'closed', c.archived_at IS NOT NULL, 'due', c.due_at, 'dateCompleted', c.completed_at, 'idList', c.list_id::text, 'idAttachmentCover', c.cover_attachment_id::text, 'idLabels', COALESCE((SELECT jsonb_agg(label_id::text) FROM card_labels WHERE card_id = c.id), '[]'::jsonb), 'attachments', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', a.id::text, 'name', a.original_name, 'mimeType', a.media_type, 'bytes', a.byte_size, 'url', COALESCE(a.external_url, '/v1/attachments/' || a.id::text || '/content'))) FROM attachments a WHERE a.card_id = c.id), '[]'::jsonb)) ORDER BY c.position), '[]'::jsonb) FROM cards c WHERE c.board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let checklists = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(jsonb_build_object('id', cl.id::text, 'name', cl.title, 'idCard', cl.card_id::text, 'pos', cl.position, 'checkItems', COALESCE((SELECT jsonb_agg(jsonb_build_object('id', ci.id::text, 'name', ci.title, 'pos', ci.position, 'state', CASE WHEN ci.is_completed THEN 'complete' ELSE 'incomplete' END) ORDER BY ci.position) FROM checklist_items ci WHERE ci.checklist_id = cl.id), '[]'::jsonb)) ORDER BY cl.position), '[]'::jsonb) FROM checklists cl INNER JOIN cards c ON c.id = cl.card_id WHERE c.board_id = $1").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    let actions = sqlx::query_scalar::<_, SqlJson<Value>>("SELECT COALESCE(jsonb_agg(payload ORDER BY created_at DESC), '[]'::jsonb) FROM (SELECT a.created_at, jsonb_build_object('id', a.id::text, 'type', a.action, 'date', a.created_at, 'data', jsonb_build_object('text', a.detail, 'card', jsonb_build_object('id', a.card_id::text)), 'memberCreator', CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('id', u.id::text, 'username', u.username) END) AS payload FROM card_activity a INNER JOIN cards c ON c.id = a.card_id LEFT JOIN users u ON u.id = a.actor_id WHERE c.board_id = $1 AND a.action <> 'Добавлен комментарий' UNION ALL SELECT cm.created_at, jsonb_build_object('id', cm.id::text, 'type', 'commentCard', 'date', cm.created_at, 'data', jsonb_build_object('text', cm.body, 'card', jsonb_build_object('id', cm.card_id::text)), 'memberCreator', CASE WHEN u.id IS NULL THEN NULL ELSE jsonb_build_object('id', u.id::text, 'username', u.username) END) AS payload FROM comments cm INNER JOIN cards c ON c.id = cm.card_id LEFT JOIN users u ON u.id = cm.author_id WHERE c.board_id = $1) exported_actions").bind(board_id).fetch_one(pool).await.map_err(ApiError::internal)?.0;
    Ok(Json(json!({ "format": "flowboard-trello-compatible/v1", "name": board.title, "prefs": { "backgroundImage": board.background_image_url }, "lists": lists, "cards": cards, "labels": labels, "checklists": checklists, "actions": actions, "members": [] })))
}

async fn create_list(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateListRequest>) -> ApiResult<ListResponse> {
    ensure_board_permission(database(&state)?, board_id, current.id, "create_lists").await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 200)?;
    let list = sqlx::query_as::<_, ListResponse>(
        "INSERT INTO lists (id, board_id, title, position) SELECT $1, b.id, $2, COALESCE((SELECT MAX(position) FROM lists WHERE board_id = b.id), 0) + 1000 FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE b.id = $3 AND m.user_id = $4 RETURNING id, title",
    )
    .bind(Uuid::new_v4()).bind(title).bind(board_id).bind(actor_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(list))
}

async fn create_label(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>, Json(request): Json<CreateLabelRequest>) -> ApiResult<LabelResponse> {
    ensure_board_permission(database(&state)?, board_id, current.id, "create_labels").await?;
    let name = valid_text(&request.name, "name", 60)?;
    let color = valid_label_color(&request.color)?;
    let label = sqlx::query_as::<_, LabelResponse>(
        "INSERT INTO labels (id, workspace_id, board_id, name, color) SELECT $1, b.workspace_id, b.id, $2, $3 FROM boards b WHERE b.id = $4 AND b.archived_at IS NULL ON CONFLICT (board_id, name) DO UPDATE SET color = EXCLUDED.color RETURNING id, name, color",
    )
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(color)
    .bind(board_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "board_not_found", "Board was not found.".to_owned()))?;
    let _ = state.events.send(());
    Ok(Json(label))
}

async fn update_list(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>, Json(request): Json<UpdateListRequest>) -> ApiResult<ListResponse> {
    ensure_list_permission(database(&state)?, list_id, current.id, "create_lists").await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 200)?;
    let list = sqlx::query_as::<_, ListResponse>(
        "UPDATE lists l SET title = $1, updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE l.id = $2 AND l.board_id = b.id AND m.user_id = $3 RETURNING l.id, l.title",
    )
    .bind(title)
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
    let board_id = sqlx::query_scalar::<_, Uuid>("SELECT board_id FROM lists WHERE id = $1 FOR UPDATE")
        .bind(list_id).fetch_optional(&mut *transaction).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    let mut list_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM lists WHERE board_id = $1 ORDER BY position, id FOR UPDATE")
        .bind(board_id).fetch_all(&mut *transaction).await.map_err(ApiError::internal)?;
    list_ids.retain(|id| *id != list_id);
    let insertion_index = match request.before_list_id {
        Some(before_list_id) if before_list_id == list_id => list_ids.len(),
        Some(before_list_id) => list_ids.iter().position(|id| *id == before_list_id).ok_or_else(|| ApiError::bad_request("Target list is unavailable."))?,
        None => list_ids.len(),
    };
    list_ids.insert(insertion_index, list_id);
    // `lists` has a UNIQUE(board_id, position) constraint. Assigning the final
    // positions one at a time can collide with a neighbour's still-current
    // position, which rolls the entire transaction back. Move every list out
    // of the positive ordering range first, then write the canonical order.
    sqlx::query("UPDATE lists SET position = -position - 1, updated_at = now() WHERE board_id = $1")
        .bind(board_id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for (index, id) in list_ids.into_iter().enumerate() {
        sqlx::query("UPDATE lists SET position = $1, updated_at = now() WHERE id = $2")
            .bind(((index + 1) as i32) * 1000).bind(id).execute(&mut *transaction).await.map_err(ApiError::internal)?;
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
    ensure_card_public_read(pool, card_id, Some(user_id)).await
}

async fn ensure_card_public_read(pool: &PgPool, card_id: Uuid, user_id: Option<Uuid>) -> Result<(), ApiError> {
    let card = sqlx::query_as::<_, (Uuid,)>(
        "SELECT c.id FROM cards c INNER JOIN boards b ON b.id = c.board_id LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE c.id = $1 AND c.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public')",
    )
    .bind(card_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?;
    card.map(|_| ()).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))
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
}

async fn load_card_comments(pool: &PgPool, card_id: Uuid, current_user_id: Option<Uuid>) -> Result<Vec<CommentResponse>, ApiError> {
    let rows = sqlx::query_as::<_, CommentRow>(
        "SELECT c.id, c.body, c.author_id, COALESCE(u.username, c.external_author_name, 'Deleted user') AS author_name, COALESCE(CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END, c.external_author_avatar_url) AS author_avatar_url, c.parent_comment_id, c.created_at::text AS created_at, c.edited_at::text AS edited_at FROM comments c LEFT JOIN users u ON u.id = c.author_id WHERE c.card_id = $1 ORDER BY c.created_at DESC, c.id DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    comment_responses(pool, rows, current_user_id).await
}

async fn comment_responses(pool: &PgPool, rows: Vec<CommentRow>, current_user_id: Option<Uuid>) -> Result<Vec<CommentResponse>, ApiError> {
    let mut comments: Vec<CommentResponse> = rows.into_iter().map(|row| CommentResponse {
        id: row.id, body: row.body, author_id: row.author_id, author_name: row.author_name, author_avatar_url: row.author_avatar_url, parent_comment_id: row.parent_comment_id,
        created_at: row.created_at, edited_at: row.edited_at, reactions: vec![],
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
    Ok(comments)
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
        "SELECT checklist_id, id, title, is_completed FROM checklist_items WHERE card_id = $1 ORDER BY position, id",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let checklists = checklist_rows.into_iter().map(|checklist| ChecklistResponse {
        id: checklist.id,
        title: checklist.title,
        items: checklist_items.iter().filter(|item| item.checklist_id == checklist.id).map(|item| ChecklistItemResponse { id: item.id, title: item.title.clone(), is_completed: item.is_completed }).collect(),
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
            }
        }
    }
    let attachments = sqlx::query_as::<_, AttachmentResponse>(
        "SELECT id, original_name, media_type, byte_size, COALESCE(external_url, '/v1/attachments/' || id::text || '/content') AS url FROM attachments WHERE card_id = $1 ORDER BY created_at DESC",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    let activity = sqlx::query_as::<_, CardActivityResponse>(
        "SELECT a.id, a.action, a.detail, COALESCE(u.username, 'Deleted user') AS actor_name, a.created_at::text AS created_at FROM card_activity a LEFT JOIN users u ON u.id = a.actor_id WHERE a.card_id = $1 ORDER BY a.created_at DESC LIMIT 100",
    )
    .bind(card_id)
    .fetch_all(pool)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(CardDetail { checklists, comments, attachments, activity, cover_attachment_id, cover_mode, background_image_url }))
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
            let valid = match type_name {
                "rectangle" | "ellipse" => number("x") && number("y") && number("width") && number("height") && number("lineWidth") && style(),
                "arrow" => number("x") && number("y") && number("x2") && number("y2") && number("lineWidth") && style(),
                "text" => number("x") && number("y") && number("fontSize") && style()
                    && element.get("text").and_then(Value::as_str).is_some_and(|value| value.len() <= 4_000)
                    && element.get("fontFamily").and_then(Value::as_str).is_some_and(|value| value.len() <= 120)
                    && element.get("fontWeight").and_then(Value::as_str).is_some_and(|value| matches!(value, "normal" | "bold")),
                "callout" => number("x") && number("y") && number("x2") && number("y2") && number("fontSize") && style()
                    && element.get("text").and_then(Value::as_str).is_some_and(|value| value.len() <= 4_000)
                    && element.get("fontFamily").and_then(Value::as_str).is_some_and(|value| value.len() <= 120)
                    && element.get("fontWeight").and_then(Value::as_str).is_some_and(|value| matches!(value, "normal" | "bold")),
                _ => false,
            };
            if !valid { return Err(ApiError::bad_request("A diagram element is invalid.")); }
        }
    }
    Ok(())
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
    ensure_card_access(pool, card_id, current.id).await?;
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

async fn delete_checklist(State(state): State<AppState>, current: CurrentUser, Path(checklist_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    let checklist = sqlx::query_as::<_, ChecklistActivityRow>("DELETE FROM checklists cl USING cards c, boards b, board_members bm WHERE cl.id = $1 AND cl.card_id = c.id AND c.board_id = b.id AND bm.board_id = b.id AND bm.user_id = $2 AND flowboard_has_permission(b.workspace_id, $2, 'edit_cards'::workspace_permission) RETURNING cl.card_id, cl.title")
        .bind(checklist_id).bind(current.id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    record_card_activity(pool, checklist.card_id, current.id, "Удалён чек-лист", &checklist.title).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn create_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(checklist_id): Path<Uuid>, Json(request): Json<CreateChecklistItemRequest>) -> ApiResult<ChecklistItemResponse> {
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 500)?;
    let pool = database(&state)?;
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "INSERT INTO checklist_items (id, checklist_id, card_id, title, position) SELECT $1, cl.id, cl.card_id, $2, COALESCE((SELECT MAX(position) FROM checklist_items WHERE checklist_id = cl.id), 0) + 1000 FROM checklists cl JOIN cards c ON c.id = cl.card_id JOIN boards b ON b.id = c.board_id JOIN board_members bm ON bm.board_id = b.id WHERE cl.id = $3 AND c.archived_at IS NULL AND bm.user_id = $4 AND flowboard_has_permission(b.workspace_id, $4, 'edit_cards'::workspace_permission) RETURNING id, card_id, title, is_completed, (SELECT title FROM checklists WHERE id = checklist_id) AS checklist_title",
    )
    .bind(Uuid::new_v4()).bind(title).bind(checklist_id).bind(actor_id)
    .fetch_optional(pool).await.map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_not_found", "Checklist was not found.".to_owned()))?;
    record_card_activity(pool, item.card_id, actor_id, "Добавлен пункт в чек-лист", &format!("{}: {}", item.checklist_title, item.title)).await;
    let _ = state.events.send(());
    Ok(Json(ChecklistItemResponse { id: item.id, title: item.title, is_completed: item.is_completed }))
}

async fn update_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(item_id): Path<Uuid>, Json(request): Json<UpdateChecklistItemRequest>) -> ApiResult<ChecklistItemResponse> {
    let actor_id = current.id;
    let pool = database(&state)?;
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "UPDATE checklist_items i SET is_completed = $1, completed_at = CASE WHEN $1 THEN now() ELSE NULL END, completed_by = CASE WHEN $1 THEN $2 ELSE NULL END FROM cards c INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE i.id = $3 AND i.card_id = c.id AND c.archived_at IS NULL AND m.user_id = $2 RETURNING i.id, i.card_id, i.title, i.is_completed, (SELECT title FROM checklists WHERE id = i.checklist_id) AS checklist_title",
    )
    .bind(request.is_completed)
    .bind(actor_id)
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    record_card_activity(pool, item.card_id, actor_id, if item.is_completed { "Отмечен пункт чек-листа" } else { "Снята отметка с пункта" }, &format!("{}: {}", item.checklist_title, item.title)).await;
    let _ = state.events.send(());
    Ok(Json(ChecklistItemResponse { id: item.id, title: item.title, is_completed: item.is_completed }))
}

async fn delete_checklist_item(State(state): State<AppState>, current: CurrentUser, Path(item_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let actor_id = current.id;
    let pool = database(&state)?;
    let item = sqlx::query_as::<_, ChecklistItemActivityRow>(
        "DELETE FROM checklist_items i USING cards c, boards b, board_members m WHERE i.id = $1 AND i.card_id = c.id AND c.board_id = b.id AND m.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $2 RETURNING i.id, i.card_id, i.title, i.is_completed, (SELECT title FROM checklists WHERE id = i.checklist_id) AS checklist_title",
    )
    .bind(item_id)
    .bind(actor_id)
    .fetch_optional(pool)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "checklist_item_not_found", "Checklist item was not found.".to_owned()))?;
    record_card_activity(pool, item.card_id, actor_id, "Удалён пункт из чек-листа", &format!("{}: {}", item.checklist_title, item.title)).await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn create_comment(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<CreateCommentRequest>) -> ApiResult<CommentResponse> {
    let actor_id = current.id;
    let body = valid_text(&request.body, "body", 10_000)?;
    let pool = database(&state)?;
    ensure_card_access(pool, card_id, actor_id).await?;
    if let Some(parent_id) = request.parent_comment_id {
        let parent_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM comments WHERE id = $1 AND card_id = $2 AND parent_comment_id IS NULL)")
            .bind(parent_id).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !parent_exists { return Err(ApiError::bad_request("Reply target is not a top-level comment on this card.")); }
    }
    let comment_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO comments (id, card_id, author_id, body, parent_comment_id) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(card_id)
    .bind(actor_id)
    .bind(body)
    .bind(request.parent_comment_id)
    .fetch_one(pool)
    .await
    .map_err(ApiError::internal)?;
    let comment = load_card_comments(pool, card_id, Some(actor_id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    record_card_activity(pool, card_id, actor_id, if comment.parent_comment_id.is_some() { "Добавлен ответ" } else { "Добавлен комментарий" }, "").await;
    let _ = state.events.send(());
    Ok(Json(comment))
}

async fn create_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Json(request): Json<CreateDiscordCardRequest>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    let source_id = valid_text(&request.source_id, "source_id", 128)?.to_owned();
    if let Some(card) = sqlx::query_as::<_, CardResponse>("SELECT id, list_id, title, description FROM cards WHERE discord_integration_id = $1 AND discord_source_id = $2")
        .bind(integration.id).bind(&source_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
        return Ok(Json(card));
    }
    let title = valid_text(&request.title, "title", 500)?;
    let description = request.description.trim();
    if description.chars().count() > 20_000 { return Err(ApiError::bad_request("description must not exceed 20000 characters.")); }
    let target_list_id = request.list_id.or(integration.default_list_id)
        .ok_or_else(|| ApiError::bad_request("list_id is required because this token has no default list."))?;
    let card = sqlx::query_as::<_, CardResponse>(
        "INSERT INTO cards (id, board_id, list_id, title, description, position, created_by, discord_integration_id, discord_source_id) SELECT $1, l.board_id, l.id, $2, $3, COALESCE((SELECT MAX(position) FROM cards WHERE list_id = l.id), 0) + 1000, NULL, $4, $5 FROM lists l INNER JOIN boards b ON b.id = l.board_id WHERE l.id = $6 AND l.board_id = $7 AND b.archived_at IS NULL RETURNING id, list_id, title, description",
    )
    .bind(Uuid::new_v4()).bind(title).bind(description).bind(integration.id).bind(&source_id).bind(target_list_id).bind(integration.board_id)
    .fetch_optional(pool).await.map_err(ApiError::internal)?;
    let card = match card {
        Some(card) => card,
        None => sqlx::query_as::<_, CardResponse>("SELECT id, list_id, title, description FROM cards WHERE discord_integration_id = $1 AND discord_source_id = $2")
            .bind(integration.id).bind(&source_id).fetch_optional(pool).await.map_err(ApiError::internal)?
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "discord_target_not_found", "Discord target list is no longer available.".to_owned()))?,
    };
    record_external_card_activity(pool, card.id, "Discord: создана задача", &card.title).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn list_discord_board_lists(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<ListResponse>> {
    let lists = sqlx::query_as::<_, ListResponse>("SELECT id, title FROM lists WHERE board_id = $1 ORDER BY position")
        .bind(integration.board_id)
        .fetch_all(database(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(lists))
}

async fn list_discord_board_cards(State(state): State<AppState>, integration: DiscordIntegration) -> ApiResult<Vec<CardResponse>> {
    let cards = sqlx::query_as::<_, CardResponse>("SELECT id, list_id, title, description FROM cards WHERE board_id = $1 AND archived_at IS NULL ORDER BY list_id, position")
        .bind(integration.board_id)
        .fetch_all(database(&state)?)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(cards))
}

async fn get_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>) -> ApiResult<DiscordCardStatusResponse> {
    let card = sqlx::query_as::<_, DiscordCardStatusResponse>("SELECT id, list_id, title, description, completed_at IS NOT NULL AS is_completed, completed_at::text AS completed_at FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL")
        .bind(card_id).bind(integration.board_id).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned()))?;
    Ok(Json(card))
}

async fn set_discord_card_completion(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardCompletionRequest>) -> ApiResult<DiscordCardStatusResponse> {
    let pool = database(&state)?;
    let card = sqlx::query_as::<_, DiscordCardStatusResponse>("UPDATE cards SET completed_at = CASE WHEN $1 THEN now() ELSE NULL END, updated_at = now() WHERE id = $2 AND board_id = $3 AND archived_at IS NULL RETURNING id, list_id, title, description, completed_at IS NOT NULL AS is_completed, completed_at::text AS completed_at")
        .bind(request.is_completed).bind(card_id).bind(integration.board_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned()))?;
    record_external_card_activity(pool, card_id, if request.is_completed { "Discord: задача выполнена" } else { "Discord: задача возвращена в работу" }, "").await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn move_discord_card(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<MoveDiscordCardRequest>) -> ApiResult<CardResponse> {
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let card = sqlx::query_as::<_, CardResponse>(
        "WITH source AS (SELECT id, board_id FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL FOR UPDATE), target AS (SELECT id, board_id FROM lists WHERE id = $3 FOR UPDATE), anchor AS (SELECT c.position FROM cards c, target, source WHERE c.id = $4 AND c.list_id = target.id AND c.id <> source.id FOR UPDATE), previous AS (SELECT c.position FROM cards c, target, source WHERE c.list_id = target.id AND c.id <> source.id AND c.position < (SELECT position FROM anchor) ORDER BY c.position DESC LIMIT 1) UPDATE cards c SET list_id = target.id, position = CASE WHEN $4 IS NULL THEN (SELECT COALESCE(MAX(position), 0) + 1000 FROM cards WHERE list_id = target.id AND id <> c.id) WHEN (SELECT position FROM previous) IS NULL THEN (SELECT position - 1000 FROM anchor) ELSE ((SELECT position FROM previous) + (SELECT position FROM anchor)) / 2 END, updated_at = now() FROM source, target WHERE c.id = source.id AND source.board_id = target.board_id AND ($4 IS NULL OR EXISTS (SELECT 1 FROM anchor)) RETURNING c.id, c.list_id, c.title, c.description",
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
    let Some(after) = query.after else { return Ok(Json(load_card_comments(pool, card_id, None).await?)); };
    let cursor_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM comments WHERE id = $1 AND card_id = $2)")
        .bind(after).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !cursor_exists { return Err(ApiError::bad_request("The comment cursor does not belong to this card.")); }
    let limit = i64::from(query.limit.unwrap_or(100).clamp(1, 200));
    let rows = sqlx::query_as::<_, CommentRow>(
        "SELECT c.id, c.body, c.author_id, COALESCE(u.username, c.external_author_name, 'Deleted user') AS author_name, COALESCE(CASE WHEN u.avatar_key IS NULL THEN NULL ELSE '/v1/avatars/' || u.id::text END, c.external_author_avatar_url) AS author_avatar_url, c.parent_comment_id, c.created_at::text AS created_at, c.edited_at::text AS edited_at FROM comments c LEFT JOIN users u ON u.id = c.author_id CROSS JOIN (SELECT created_at, id FROM comments WHERE id = $2 AND card_id = $1) anchor WHERE c.card_id = $1 AND (c.created_at, c.id) > (anchor.created_at, anchor.id) ORDER BY c.created_at ASC, c.id ASC LIMIT $3",
    )
    .bind(card_id).bind(after).bind(limit)
    .fetch_all(pool).await.map_err(ApiError::internal)?;
    Ok(Json(comment_responses(pool, rows, None).await?))
}

async fn set_discord_card_cover(State(state): State<AppState>, integration: DiscordIntegration, Path(card_id): Path<Uuid>, Json(request): Json<SetDiscordCardCoverRequest>) -> ApiResult<Value> {
    if request.mode != "full" && request.mode != "top" { return Err(ApiError::bad_request("Cover mode must be full or top.")); }
    if request.attachment_id.is_some() == request.attachment_url.as_deref().map(str::trim).filter(|url| !url.is_empty()).is_some() {
        return Err(ApiError::bad_request("Provide exactly one of attachment_id or attachment_url."));
    }
    let pool = database(&state)?;
    let attachment_id = if let Some(attachment_id) = request.attachment_id {
        let is_valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attachments a INNER JOIN cards c ON c.id = a.card_id WHERE a.id = $1 AND a.card_id = $2 AND c.board_id = $3 AND c.archived_at IS NULL AND a.media_type LIKE 'image/%')")
            .bind(attachment_id).bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !is_valid { return Err(ApiError::bad_request("Attachment must be an image on this card in the token's board.")); }
        attachment_id
    } else {
        let attachment_url = request.attachment_url.as_deref().unwrap().trim();
        sqlx::query_scalar::<_, Uuid>("SELECT a.id FROM attachments a INNER JOIN cards c ON c.id = a.card_id WHERE a.card_id = $1 AND c.board_id = $2 AND c.archived_at IS NULL AND a.external_url = $3 AND a.media_type LIKE 'image/%' ORDER BY a.created_at DESC LIMIT 1")
            .bind(card_id).bind(integration.board_id).bind(attachment_url).fetch_optional(pool).await.map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::bad_request("Image attachment URL was not found on this card."))?
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
    let archived = sqlx::query_scalar::<_, Uuid>("UPDATE cards SET archived_at = now(), updated_at = now() WHERE id = $1 AND board_id = $2 AND archived_at IS NULL RETURNING id")
        .bind(card_id).bind(integration.board_id).fetch_optional(pool).await.map_err(ApiError::internal)?;
    if archived.is_some() {
        record_external_card_activity(pool, card_id, "Discord: предложка архивирована", "").await;
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
    if let Some(comment_id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM comments WHERE discord_integration_id = $1 AND discord_message_id = $2")
        .bind(integration.id).bind(&message_id).fetch_optional(pool).await.map_err(ApiError::internal)? {
        let comment = load_card_comments(pool, card_id, None).await?.into_iter().find(|comment| comment.id == comment_id)
            .ok_or_else(|| ApiError::bad_request("Discord comment belongs to another card."))?;
        return Ok(Json(comment));
    }
    let card_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cards WHERE id = $1 AND board_id = $2 AND archived_at IS NULL)")
        .bind(card_id).bind(integration.board_id).fetch_one(pool).await.map_err(ApiError::internal)?;
    if !card_exists { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found on this Discord integration board.".to_owned())); }
    let author_name = valid_text(&request.author_name, "author_name", 120)?.to_owned();
    let author_avatar_url = request.author_avatar_url.as_deref().map(valid_discord_asset_url).transpose()?.map(ToOwned::to_owned);
    let mut attachment_rows = Vec::new();
    let mut parts = Vec::new();
    if !request.body.trim().is_empty() { parts.push(request.body.trim().to_owned()); }
    for attachment in &request.attachments {
        let url = valid_discord_asset_url(&attachment.url)?.to_owned();
        let filename = valid_text(&attachment.filename, "attachment filename", 255)?.to_owned();
        if !matches!(attachment.media_type.as_str(), "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "video/mp4" | "video/webm" | "video/quicktime") {
            return Err(ApiError::bad_request("Discord attachments must be JPEG, PNG, GIF, WebP, MP4, WebM, or MOV."));
        }
        if !(0..=50 * 1024 * 1024).contains(&attachment.byte_size) { return Err(ApiError::bad_request("Discord attachment size must be between 0 and 50 MiB.")); }
        parts.push(discord_media_markdown(&filename, &attachment.media_type, &url));
        attachment_rows.push((url, filename, attachment.media_type.clone(), attachment.byte_size));
    }
    let body = valid_text(&parts.join("\n"), "body", 10_000)?.to_owned();
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let comment_id = Uuid::new_v4();
    sqlx::query("INSERT INTO comments (id, card_id, author_id, body, external_author_name, external_author_avatar_url, discord_integration_id, discord_message_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7)")
        .bind(comment_id).bind(card_id).bind(&body).bind(&author_name).bind(&author_avatar_url).bind(integration.id).bind(&message_id)
        .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    for (url, filename, media_type, byte_size) in attachment_rows {
        sqlx::query("INSERT INTO attachments (id, card_id, uploaded_by, object_key, original_name, media_type, byte_size, external_url) VALUES ($1, $2, NULL, NULL, $3, $4, $5, $6)")
            .bind(Uuid::new_v4()).bind(card_id).bind(filename).bind(media_type).bind(byte_size).bind(url)
            .execute(&mut *transaction).await.map_err(ApiError::internal)?;
    }
    transaction.commit().await.map_err(ApiError::internal)?;
    let comment = load_card_comments(pool, card_id, None).await?.into_iter().find(|comment| comment.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Discord comment could not be loaded."))?;
    record_external_card_activity(pool, card_id, "Discord: добавлен комментарий", &author_name).await;
    let _ = state.events.send(());
    Ok(Json(comment))
}

async fn update_comment(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>, Json(request): Json<UpdateCommentRequest>) -> ApiResult<CommentResponse> {
    let body = valid_text(&request.body, "body", 10_000)?;
    let card_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE comments c SET body = $1, edited_at = now() FROM cards card INNER JOIN boards b ON b.id = card.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $2 AND c.card_id = card.id AND c.author_id = $3 AND m.user_id = $3 AND card.archived_at IS NULL RETURNING c.card_id",
    )
    .bind(body)
    .bind(comment_id)
    .bind(current.id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::forbidden("Only the comment author can edit it."))?;
    let comment = load_card_comments(database(&state)?, card_id, Some(current.id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    let _ = state.events.send(());
    Ok(Json(comment))
}

async fn toggle_comment_reaction(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>, Json(request): Json<ToggleCommentReactionRequest>) -> ApiResult<Vec<CommentReactionResponse>> {
    let emoji = request.emoji.trim();
    if emoji.is_empty() || emoji.chars().count() > 16 { return Err(ApiError::bad_request("Reaction emoji must be between 1 and 16 characters.")); }
    let pool = database(&state)?;
    let card_id = sqlx::query_scalar::<_, Uuid>("SELECT card_id FROM comments WHERE id = $1")
        .bind(comment_id).fetch_optional(pool).await.map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::bad_request("Comment was not found."))?;
    ensure_card_access(pool, card_id, current.id).await?;
    let removed = sqlx::query("DELETE FROM comment_reactions WHERE comment_id = $1 AND user_id = $2 AND emoji = $3")
        .bind(comment_id).bind(current.id).bind(emoji).execute(pool).await.map_err(ApiError::internal)?;
    if removed.rows_affected() == 0 {
        sqlx::query("INSERT INTO comment_reactions (comment_id, user_id, emoji) VALUES ($1, $2, $3)")
            .bind(comment_id).bind(current.id).bind(emoji).execute(pool).await.map_err(ApiError::internal)?;
    }
    let comment = load_card_comments(pool, card_id, Some(current.id)).await?.into_iter().find(|item| item.id == comment_id)
        .ok_or_else(|| ApiError::bad_request("Comment could not be loaded."))?;
    let _ = state.events.send(());
    Ok(Json(comment.reactions))
}

async fn delete_comment(State(state): State<AppState>, current: CurrentUser, Path(comment_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let result = sqlx::query(
        "DELETE FROM comments c USING cards card, boards b, board_members m WHERE c.id = $1 AND c.card_id = card.id AND card.board_id = b.id AND m.board_id = b.id AND m.user_id = $2 AND c.author_id = $2",
    )
    .bind(comment_id)
    .bind(current.id)
    .execute(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError::forbidden("You cannot delete this comment.")); }
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
    let extension = attachment_extension(&media_type, &original_name).ok_or_else(|| ApiError::bad_request("Only JPEG, PNG, GIF, WebP, MP4, WebM, and MOV files are supported."))?;
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
        Ok(attachment) => Ok(Json(attachment)),
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            Err(ApiError::internal(error))
        }
    }
}

async fn delete_attachment(State(state): State<AppState>, current: CurrentUser, Path(attachment_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    let actor_id = current.id;
    let object_key = sqlx::query_scalar::<_, Option<String>>(
        "DELETE FROM attachments a USING cards c, boards b, board_members m WHERE a.id = $1 AND a.card_id = c.id AND c.board_id = b.id AND c.archived_at IS NULL AND m.board_id = b.id AND m.user_id = $2 RETURNING a.object_key",
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
    Ok(StatusCode::NO_CONTENT)
}

async fn download_attachment(State(state): State<AppState>, current: Viewer, Path(attachment_id): Path<Uuid>) -> Result<Response, ApiError> {
    let attachment = sqlx::query_as::<_, (Option<String>, String, Option<String>)>(
        "SELECT a.object_key, a.media_type, a.external_url FROM attachments a INNER JOIN cards c ON c.id = a.card_id INNER JOIN boards b ON b.id = c.board_id LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE a.id = $1 AND c.archived_at IS NULL AND (m.user_id IS NOT NULL OR b.visibility = 'public')",
    )
    .bind(attachment_id)
    .bind(current.0.map(|user| user.id))
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment was not found.".to_owned()))?;
    if let Some(url) = attachment.2 { return Ok((StatusCode::FOUND, [(header::LOCATION, HeaderValue::from_str(&url).map_err(|_| ApiError::storage())?)]).into_response()); }
    let object_key = attachment.0.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found.".to_owned()))?;
    let bytes = tokio::fs::read(state.upload_dir.join(object_key)).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ApiError(StatusCode::NOT_FOUND, "attachment_not_found", "Attachment file was not found.".to_owned())
        } else {
            tracing::error!(?error, "attachment read failed");
            ApiError::storage()
        }
    })?;
    let content_type = HeaderValue::from_str(&attachment.1).map_err(|_| ApiError::storage())?;
    Ok(([(header::CONTENT_TYPE, content_type), (header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"))], bytes).into_response())
}

async fn create_card(State(state): State<AppState>, current: CurrentUser, Path(list_id): Path<Uuid>, Json(request): Json<CreateCardRequest>) -> ApiResult<CardResponse> {
    ensure_list_permission(database(&state)?, list_id, current.id, "create_cards").await?;
    let actor_id = current.id;
    let title = valid_text(&request.title, "title", 500)?;
    let description = request.description.trim();
    if description.chars().count() > 20_000 { return Err(ApiError::bad_request("description must not exceed 20000 characters.")); }
    let card = sqlx::query_as::<_, CardResponse>(
        "INSERT INTO cards (id, board_id, list_id, title, description, position, created_by) SELECT $1, l.board_id, l.id, $2, $3, COALESCE((SELECT MAX(position) FROM cards WHERE list_id = l.id), 0) + 1000, $4 FROM lists l INNER JOIN boards b ON b.id = l.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE l.id = $5 AND m.user_id = $4 RETURNING id, list_id, title, description",
    )
    .bind(Uuid::new_v4()).bind(title).bind(description).bind(actor_id).bind(list_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "list_not_found", "List was not found.".to_owned()))?;
    record_card_activity(database(&state)?, card.id, actor_id, "Создана задача", &card.title).await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn update_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardRequest>) -> ApiResult<CardResponse> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
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
    if title.is_none() && description.is_none() { return Err(ApiError::bad_request("At least one editable field is required.")); }
    let card = sqlx::query_as::<_, CardResponse>(
        "UPDATE cards c SET title = COALESCE($1, c.title), description = COALESCE($2, c.description), updated_at = now() FROM boards b INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $3 AND c.board_id = b.id AND c.archived_at IS NULL AND m.user_id = $4 RETURNING c.id, c.list_id, c.title, c.description",
    )
    .bind(title)
    .bind(description)
    .bind(card_id)
    .bind(actor_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned()))?;
    record_card_activity(database(&state)?, card.id, actor_id, "Изменена задача", "Название или описание").await;
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
        let is_image: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attachments WHERE id = $1 AND card_id = $2 AND media_type LIKE 'image/%')")
            .bind(attachment_id).bind(card_id).fetch_one(pool).await.map_err(ApiError::internal)?;
        if !is_image { return Err(ApiError::bad_request("Card cover must be an image attached to this card.")); }
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
    let background = sqlx::query_as::<_, (String, String)>("SELECT cb.object_key, cb.media_type FROM card_backgrounds cb INNER JOIN cards c ON c.id = cb.card_id INNER JOIN boards b ON b.id = c.board_id LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE cb.card_id = $1 AND c.archived_at IS NULL AND (b.visibility = 'public' OR m.user_id IS NOT NULL)")
        .bind(card_id).bind(current.0.map(|user| user.id)).fetch_optional(database(&state)?).await.map_err(ApiError::internal)?
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
    let is_visible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM boards b WHERE b.id = $1 AND b.archived_at IS NULL AND b.visibility = 'public' AND (EXISTS(SELECT 1 FROM board_members bm WHERE bm.board_id = b.id AND bm.user_id = $2) OR EXISTS(SELECT 1 FROM card_assignees ca INNER JOIN cards c ON c.id = ca.card_id WHERE c.board_id = b.id AND c.archived_at IS NULL AND ca.user_id = $2) OR EXISTS(SELECT 1 FROM comments cm INNER JOIN cards c ON c.id = cm.card_id WHERE c.board_id = b.id AND cm.author_id = $2)))")
        .bind(board_id).bind(user_id).fetch_one(database(&state)?).await.map_err(ApiError::internal)?;
    if !is_visible { return Err(ApiError(StatusCode::NOT_FOUND, "avatar_not_found", "Avatar was not found.".to_owned())); }
    avatar_response(&state, user_id).await
}

async fn update_card_completion(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<UpdateCardCompletionRequest>) -> Result<StatusCode, ApiError> {
    let pool = database(&state)?;
    ensure_card_permission(pool, card_id, current.id, "edit_cards").await?;
    let result = sqlx::query("UPDATE cards SET completed_at = CASE WHEN $1 THEN now() ELSE NULL END, updated_at = now() WHERE id = $2 AND archived_at IS NULL")
        .bind(request.is_completed).bind(card_id).execute(pool).await.map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(pool, card_id, current.id, if request.is_completed { "Задача выполнена" } else { "Задача возвращена в работу" }, "").await;
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
    let labels = sqlx::query_as::<_, LabelResponse>("SELECT l.id, l.name, l.color FROM card_labels cl INNER JOIN labels l ON l.id = cl.label_id WHERE cl.card_id = $1 ORDER BY l.name")
        .bind(card_id)
        .fetch_all(&mut *transaction)
    .await
    .map_err(ApiError::internal)?;
    transaction.commit().await.map_err(ApiError::internal)?;
    let _ = state.events.send(());
    Ok(Json(labels))
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
    let _ = state.events.send(());
    Ok(Json(members))
}

async fn archive_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> Result<StatusCode, ApiError> {
    ensure_card_permission(database(&state)?, card_id, current.id, "delete_cards").await?;
    let actor_id = current.id;
    let result = sqlx::query(
        "UPDATE cards c SET archived_at = now(), updated_at = now() FROM boards b WHERE c.id = $1 AND c.board_id = b.id AND c.archived_at IS NULL AND b.archived_at IS NULL",
    )
    .bind(card_id)
    .execute(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 { return Err(ApiError(StatusCode::NOT_FOUND, "card_not_found", "Card was not found.".to_owned())); }
    record_card_activity(database(&state)?, card_id, actor_id, "Задача архивирована", "").await;
    let _ = state.events.send(());
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>) -> ApiResult<CardResponse> {
    ensure_archived_card_permission(database(&state)?, card_id, current.id, "delete_cards").await?;
    let card = sqlx::query_as::<_, CardResponse>(
        "UPDATE cards c SET archived_at = NULL, updated_at = now() FROM boards b WHERE c.id = $1 AND c.board_id = b.id AND c.archived_at IS NOT NULL AND b.archived_at IS NULL RETURNING c.id, c.list_id, c.title, c.description",
    )
    .bind(card_id)
    .fetch_optional(database(&state)?)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "archived_card_not_found", "Archived card was not found.".to_owned()))?;
    record_card_activity(database(&state)?, card.id, current.id, "Задача восстановлена", "").await;
    let _ = state.events.send(());
    Ok(Json(card))
}

async fn list_archived_cards(State(state): State<AppState>, current: CurrentUser, Path(board_id): Path<Uuid>) -> ApiResult<Vec<ArchivedCardResponse>> {
    let cards = sqlx::query_as::<_, ArchivedCardResponse>(
        "SELECT c.id, c.list_id, c.title, c.description, c.archived_at::text AS archived_at FROM cards c INNER JOIN boards b ON b.id = c.board_id LEFT JOIN board_members m ON m.board_id = b.id AND m.user_id = $2 WHERE c.board_id = $1 AND c.archived_at IS NOT NULL AND (m.user_id IS NOT NULL OR EXISTS (SELECT 1 FROM users u WHERE u.id = $2 AND u.is_system_owner AND u.disabled_at IS NULL) OR EXISTS (SELECT 1 FROM workspace_members wm WHERE wm.workspace_id = b.workspace_id AND wm.user_id = $2 AND wm.role IN ('owner', 'full_access'))) ORDER BY c.archived_at DESC",
    )
    .bind(board_id)
    .bind(current.id)
    .fetch_all(database(&state)?)
    .await
    .map_err(ApiError::internal)?;
    Ok(Json(cards))
}

async fn move_card(State(state): State<AppState>, current: CurrentUser, Path(card_id): Path<Uuid>, Json(request): Json<MoveCardRequest>) -> ApiResult<CardResponse> {
    ensure_card_permission(database(&state)?, card_id, current.id, "edit_cards").await?;
    let actor_id = current.id;
    let pool = database(&state)?;
    let mut transaction = pool.begin().await.map_err(ApiError::internal)?;
    let card = sqlx::query_as::<_, CardResponse>(
        "WITH source AS (SELECT c.id, c.board_id FROM cards c INNER JOIN boards b ON b.id = c.board_id INNER JOIN board_members m ON m.board_id = b.id WHERE c.id = $1 AND c.archived_at IS NULL AND m.user_id = $4 FOR UPDATE), target AS (SELECT id, board_id FROM lists WHERE id = $2 FOR UPDATE), anchor AS (SELECT c.position FROM cards c, target, source WHERE c.id = $3 AND c.list_id = target.id AND c.id <> source.id FOR UPDATE), previous AS (SELECT c.position FROM cards c, target, source WHERE c.list_id = target.id AND c.id <> source.id AND c.position < (SELECT position FROM anchor) ORDER BY c.position DESC LIMIT 1) UPDATE cards c SET list_id = target.id, position = CASE WHEN $3 IS NULL THEN (SELECT COALESCE(MAX(position), 0) + 1000 FROM cards WHERE list_id = target.id AND id <> c.id) WHEN (SELECT position FROM previous) IS NULL THEN (SELECT position - 1000 FROM anchor) ELSE ((SELECT position FROM previous) + (SELECT position FROM anchor)) / 2 END, updated_at = now() FROM source, target WHERE c.id = source.id AND source.board_id = target.board_id AND ($3 IS NULL OR EXISTS (SELECT 1 FROM anchor)) RETURNING c.id, c.list_id, c.title, c.description",
    )
    .bind(card_id)
    .bind(request.target_list_id)
    .bind(request.before_card_id)
    .bind(actor_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "card_or_list_not_found", "Card or target list was not found.".to_owned()))?;
    transaction.commit().await.map_err(ApiError::internal)?;
    record_card_activity(pool, card.id, actor_id, "Перемещена задача", "Изменена колонка или порядок").await;
    let _ = state.events.send(());
    Ok(Json(card))
}
