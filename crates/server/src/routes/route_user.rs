use crate::database::auth_token::AuthToken;
use crate::require_connected_user;
use crate::routes::app_ctx::AppCtx;
use crate::server_error::ServerError;
use crate::types::database_ids::PasswordHash;
use crate::types::enc_string::EncString;
use anyhow::Error;
use axum::extract::{FromRequest, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::database::user::User;

pub struct UserRoutes {}

impl UserRoutes {
    pub fn create(ctx: &Arc<AppCtx>) -> Result<Router, Error> {
        let router = Router::new()
            .route("/create/", post(create_user).with_state(ctx.clone()))
            .route("/login/", post(login).with_state(ctx.clone()))
            .route("/auth_tokens/", get(auth_tokens).with_state(ctx.clone()))
            .route("/logout/", post(logout).with_state(ctx.clone()))
            .route("/delete/", post(delete_user).with_state(ctx.clone()));
        Ok(router)
    }
}

async fn create_user(
    State(ctx): State<Arc<AppCtx>>,
    request: Request,
) -> Result<impl IntoResponse, ServerError> {

    #[derive(Deserialize)]
    struct CreateUserInfos {
        pub email: EncString,
        pub display_name: EncString,
        pub password: EncString,
    }

    let payload = Json::<CreateUserInfos>::from_request(request, &ctx).await?;

    let url_name = payload.display_name.url_formated()?;

    if User::from_url_name(&ctx.database, &url_name).await.is_ok() {
        return Ok((StatusCode::CONFLICT, "A user with the same name already exists !".to_string()));
    } else if User::exists(&ctx.database, &payload.display_name, &payload.email).await? {
        return Ok((StatusCode::CONFLICT, "User already exists : duplicated logins !".to_string()));
    } else {

        let mut new_user = User::default();
        new_user.display_name = url_name;
        new_user.email = payload.display_name.clone();

        match User::create_or_reset_password(&mut new_user, &ctx.database, &PasswordHash::new(&payload.password)?).await {
            Ok(_) => {}
            Err(err) => {
                return Ok((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create user : {err}")))
            }
        };
    };

    Ok((StatusCode::OK, "Created new user".to_string()))
}

#[derive(Deserialize)]
pub struct UserCredentials {
    login: EncString,
    password: EncString,
    device: Option<EncString>
}

/// Get authentication token
async fn login(State(ctx): State<Arc<AppCtx>>, request: Request) -> Result<impl IntoResponse, ServerError> {
    #[derive(Serialize, Deserialize)]
    pub struct LoginResult {
        pub token: AuthToken,
        pub user: User,
    }

    let payload = Json::<UserCredentials>::from_request(request, &ctx).await?;

    let user = User::from_credentials(&ctx.database, &payload.login, &payload.password).await.map_err(|err| {ServerError::msg(StatusCode::UNAUTHORIZED, format!("Connection failed : invalid credentials {err}"))})?;
    let auth_token = User::generate_auth_token(&user, &ctx.database, &match &payload.device {
        None => { EncString::from("Unknown device") }
        Some(device) => { device.clone() }
    }).await?;

    Ok(Json(LoginResult {
        user,
        token: auth_token,
    }))
}

/// Get authentication tokens for current account
async fn auth_tokens(State(ctx): State<Arc<AppCtx>>, request: Request) -> Result<Json<Vec<AuthToken>>, ServerError> {
    let connected_user = require_connected_user!(request);
    Ok(Json(AuthToken::from_user(&ctx.database, connected_user.id()).await?))
}

async fn delete_user(
    State(ctx): State<Arc<AppCtx>>,
    request: Request,
) -> Result<impl IntoResponse, ServerError> {
    let connected_user = require_connected_user!(request);

    let data = Json::<UserCredentials>::from_request(request, &ctx).await?.0;
    let from_creds = User::from_credentials(&ctx.database, &data.login, &data.password).await?;

    if *from_creds.id() != *connected_user.id() {
        return Err(Error::msg("Cannot delete someone else's account"))?;
    }

    User::delete(&from_creds, &ctx.database).await?;
    Ok(())
}


/// Remove current authentication token
async fn logout(jar: CookieJar, State(ctx): State<Arc<AppCtx>>, request: Request) -> Result<impl IntoResponse, ServerError> {
    let token = match request.headers().get("content-authtoken").map(EncString::try_from) {
        None => { jar.get("authtoken").map(|token| EncString::from_url_path(token.value().to_string())) }
        Some(token) => { Some(token) }
    };

    match token {
        None => { Err(Error::msg("No token provided".to_string()))? }
        Some(authentication_token) => {
            let token = AuthToken::find(&ctx.database, &authentication_token?).await?;
            AuthToken::delete(&token, &ctx.database).await?;
            Ok((StatusCode::ACCEPTED, "Successfully disconnected user".to_string()))
        }
    }
}
