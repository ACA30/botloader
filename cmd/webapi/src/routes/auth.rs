use std::sync::Arc;

use axum::{
    extract::{self, Query},
    http::{header::LOCATION, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse},
};
use oauth2::{reqwest::async_http_client, AuthorizationCode, Scope, TokenResponse};
use tracing::{error, info, instrument};

use crate::{errors::ApiErrorResponse, middlewares::LoggedInSession, ApiResult, ConfigData};

use stores::web::{CsrfStore, SessionStore};

pub struct AuthHandlers<CT, ST> {
    session_store: ST,
    csrf_store: CT,
}

#[derive(serde::Deserialize)]
pub struct ConfirmLoginQuery {
    code: String,
    state: String,
}
impl<CT, ST> AuthHandlers<CT, ST> {
    pub fn new(session_store: ST, csrf_store: CT) -> Self {
        Self {
            csrf_store,
            session_store,
        }
    }
}

impl<CT: CsrfStore, ST: SessionStore> AuthHandlers<CT, ST> {
    #[instrument(skip(auth_handler, conf))]
    pub async fn handle_login(
        auth_handler: extract::Extension<Arc<AuthHandlers<CT, ST>>>,
        conf: extract::Extension<ConfigData>,
    ) -> ApiResult<impl IntoResponse> {
        let token = auth_handler
            .csrf_store
            .generate_csrf_token()
            .await
            .map_err(|err| {
                error!(%err, "failed creating csrf token");
                ApiErrorResponse::InternalError
            })?;

        // Generate the full authorization URL.
        let (auth_url, _) = conf
            .oauth_client
            .authorize_url(|| token)
            // Set the desired scopes.
            .add_scope(Scope::new("identify".to_string()))
            .add_scope(Scope::new("guilds".to_string()))
            // Set the PKCE code challenge.
            // .set_pkce_challenge(pkce_challenge)
            // TODO: Do we need to use pkce challenges? wouldn't it be enough to verify the "state" parameter alone?
            .url();

        let mut headers = HeaderMap::new();
        headers.insert(
            LOCATION,
            HeaderValue::from_str(&auth_url.to_string()).unwrap(),
        );
        Ok((StatusCode::SEE_OTHER, headers))
    }

    #[instrument(skip(auth_handler, conf, data))]
    pub async fn handle_confirm_login(
        auth_handler: extract::Extension<Arc<AuthHandlers<CT, ST>>>,
        conf: extract::Extension<ConfigData>,
        Query(data): Query<ConfirmLoginQuery>,
    ) -> ApiResult<impl IntoResponse> {
        let valid_csrf_token = auth_handler
            .csrf_store
            .check_csrf_token(&data.state)
            .await
            .map_err(|err| {
                error!(%err, "failed checking csrf token");
                ApiErrorResponse::InternalError
            })?;

        if !valid_csrf_token {
            return Err(ApiErrorResponse::BadCsrfToken);
        }

        let token_result = conf
            .oauth_client
            .exchange_code(AuthorizationCode::new(data.code))
            // Set the PKCE code verifier.
            .request_async(async_http_client)
            .await
            .map_err(|err| {
                error!(%err, "failed exchanging oauth2 code");
                ApiErrorResponse::InternalError
            })?;

        let access_token = token_result.access_token();
        let client = twilight_http::Client::new(format!("Bearer {}", access_token.secret()));
        let user = client
            .current_user()
            .exec()
            .await
            .map_err(|err| {
                error!(%err, "discord api request failed, failed getting current user");
                ApiErrorResponse::InternalError
            })?
            .model()
            .await
            .map_err(|err| {
                error!(%err, "failed reading/decoding discord response body");
                ApiErrorResponse::InternalError
            })?;

        let session = auth_handler
            .session_store
            .create_session(user, token_result)
            .await
            .map_err(|err| {
                error!(%err, "failed creating user sessionø");
                ApiErrorResponse::InternalError
            })?;

        Ok(Html(format!(
            "
        <html>
        <body>Login successfull! Token: {}</body>
        </html>",
            session.token,
        )))
    }

    #[instrument(skip(auth_handler, session))]
    pub async fn handle_logout(
        auth_handler: extract::Extension<Arc<AuthHandlers<CT, ST>>>,
        session: extract::Extension<LoggedInSession>,
    ) -> ApiResult<impl IntoResponse> {
        auth_handler
            .session_store
            .del_session(&session.raw.token)
            .await
            .map_err(|err| {
                error!(%err, "failed deleting sesison");
                ApiErrorResponse::InternalError
            })?;

        info!("Logged out a user");

        Ok(Html(format!(
            "
        <html>
        <body>Logout successfull! {}</body>
        </html>",
            session.raw.user.name
        )))
    }
}
