mod static_file_server;

use std::{env, fs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use anyhow::Error;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::{middleware, Router};
use axum::routing::{get};
use serde::{Deserialize, Serialize};
use tracing::{info};
use which::which;
use crate::config::WebClientConfig;
use crate::database::calendar::Calendar;
use crate::{get_connected_user, get_display_calendar};
use crate::database::calendar_users::CalendarUser;
use crate::database::user::User;
use crate::routes::app_ctx::AppCtx;
use crate::routes::RequestContext;
use crate::server_error::ServerError;
use crate::types::enc_string::EncString;
use crate::web_client::static_file_server::StaticFileServer;

pub struct WebClient {
    _subcommand: Option<Child>,
}

impl WebClient {
    pub async fn new(config: &WebClientConfig) -> Result<Self, Error> {
        let base_directory = env::current_dir()?;
        let client = Self::try_create_client(config).await;
        env::set_current_dir(base_directory)?;
        client
    }

    async fn try_create_client(config: &WebClientConfig) -> Result<Self, Error> {
        if config.build_webpack {
            env::set_current_dir(&config.client_path)?;

            let result = which("node").or(Err(Error::msg("Failed to find node path. Please ensure nodejs is correctly installed")))?;
            let mut npm_cli_path = result.parent().unwrap().join("node_modules").join("npm").join("bin").join("npm-cli.js");
            if !npm_cli_path.exists() {
                npm_cli_path = PathBuf::from("/usr/lib/node_modules/npm/bin/npm-cli.js");
            }
            if !npm_cli_path.exists() {
                return Err(Error::msg("NPM cli does not exist"));
            }


            if config.check_for_packages_updates {
                info!("Installing webclient dependencies...");
                let mut install_cmd = Command::new("node")
                    .arg(npm_cli_path.to_str().unwrap())
                    .arg("install")
                    .stderr(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .spawn()?;
                install_cmd.wait()?;
                info!("Installed webclient dependencies !");
            }

            let command = if config.debug
            {
                Command::new("node")
                    .arg(npm_cli_path.to_str().unwrap())
                    .arg("run")
                    .arg("dev")
                    .stderr(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .spawn()?
            } else {
                Command::new("node")
                    .arg(npm_cli_path.to_str().unwrap())
                    .arg("run")
                    .arg("prod")
                    .stderr(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .spawn()?
            };
            Ok(Self { _subcommand: Some(command) })
        } else {
            Ok(Self { _subcommand: None })
        }
    }

    pub fn router(ctx: &Arc<AppCtx>) -> Result<Router, Error> {
        Ok(Router::new()
            .route("/", get(get_index).with_state(ctx.clone()))
            .route("/{display_calendar}/", get(get_index).with_state(ctx.clone()))
            .route("/favicon.ico", get(Self::get_favicon).with_state(ctx.clone()))
            .nest("/public/", StaticFileServer::router(ctx.config.web_client_config.client_path.join("public")))
            .layer(middleware::from_fn_with_state(ctx.clone(), middleware_get_path_context))
        )
    }

    async fn get_favicon(State(ctx): State<Arc<AppCtx>>) -> Result<impl IntoResponse, ServerError> {
        StaticFileServer::serve_file_from_path(ctx.config.web_client_config.client_path.join("public").join("images").join("icons").join("favicon.ico")).await
    }
}

#[derive(Deserialize, Debug)]
pub struct PathData {
    display_calendar: Option<String>,
}

pub async fn middleware_get_path_context(State(ctx): State<Arc<AppCtx>>, Path(PathData { display_calendar }): Path<PathData>, request: axum::http::Request<Body>, next: Next) -> Result<Response, ServerError> {
    let context = request.extensions().get::<Arc<RequestContext>>().unwrap();
    context.is_web_client.store(true, SeqCst);
    if let Some(display_calendar) = display_calendar {
        if let Ok(display_calendar) = Calendar::from_key(&ctx.database, &EncString::from_url_path(display_calendar.clone())?).await {
            *context.display_calendar.write().await = Some(display_calendar);
        } else {
            return Err(ServerError::msg(StatusCode::NOT_FOUND, format!("Unknown user '{}'", display_calendar)));
        }
    }
    Ok(next.run(request).await)
}

#[derive(Serialize, Default)]
struct ClientAppConfig {
    pub origin: String,
    pub connected_user: Option<User>,
    pub display_calendar: Option<Calendar>,
    pub display_calendar_users: Option<Vec<CalendarUser>>,
}

pub fn get_origin(ctx: &Arc<AppCtx>, request: &Request) -> Result<String, ServerError> {
    let use_https = if let Some(scheme) = request.uri().scheme_str() { scheme == "https" } else { ctx.config.use_tls || ctx.config.web_client_config.force_secure_requests };
    Ok(format!("{}://{}", if use_https { "https" } else { "http" }, match request.headers().get("host") {
        None => {
            match request.uri().host() {
                None => { ctx.config.addresses[0].to_string() }
                Some(host) => {
                    match request.uri().port() {
                        None => { ctx.config.addresses[0].to_string() }
                        Some(port) => { format!("{}:{}", host, port.as_str()) }
                    }
                }
            }
        }
        Some(host) => { host.to_str()?.to_string() }
    }))
}


async fn get_index(State(ctx): State<Arc<AppCtx>>, request: Request) -> Result<impl IntoResponse, ServerError> {
    let mut client_config = ClientAppConfig {
        origin: get_origin(&ctx, &request)?,
        ..Default::default()
    };

    get_connected_user!(request, user, {
        client_config.connected_user = Some(user.clone());
    });
    get_display_calendar!(request, repository, {
        client_config.display_calendar_users = Some(CalendarUser::from_calendar(&ctx.database, repository.id()).await?);
        client_config.display_calendar = Some(repository.clone());
    });

    let index_path_buf = ctx.config.web_client_config.client_path.join("public").join("index.html");
    let index_path = index_path_buf.to_str().unwrap();
    let index_data = match fs::read_to_string(index_path) {
        Ok(file) => { file }
        Err(err) => { Err(Error::msg(format!("Cannot find index file : {err} (searching in {index_path})")))? }
    };
    let index_data = index_data.replace(r#"data-app_config='{}'"#, format!(r##"data-app_config='{}'"##, serde_json::to_string(&client_config)?).as_str());
    Ok(Html(index_data))
}