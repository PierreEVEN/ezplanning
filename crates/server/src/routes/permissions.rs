use axum::extract::Request;
use axum::http::StatusCode;
use std::sync::Arc;
use crate::database::Database;
use crate::database::item::{DbItem, Trash};
use crate::database::repository::DbRepository;
use crate::database::subscription::{Subscription, SubscriptionAccessType};
use crate::routes::RequestContext;
use crate::server_error::ServerError;
use crate::types::database_ids::{ItemId, RepositoryId};
use crate::types::repository::RepositoryStatus;

pub struct Permissions {
    request_context: Arc<RequestContext>,
}

unsafe impl Send for Permissions {}
unsafe impl Sync for Permissions {}

pub enum PermissionResult {
    Granted,
    Denied,
}

impl PermissionResult {
    pub fn require(&self) -> Result<(), ServerError> {
        match self {
            PermissionResult::Denied => { Err(ServerError::msg(StatusCode::FORBIDDEN, "Access denied")) }
            _ => { Ok(()) }
        }
    }

    pub fn granted(&self) -> bool {
        match self {
            PermissionResult::Granted => { true }
            _ => { false }
        }
    }
}

impl Permissions {
    pub fn new(request: &Request) -> Result<Self, ServerError> {
        Ok(Self {
            request_context: request.extensions().get::<Arc<RequestContext>>().unwrap().clone(),
        })
    }

    pub async fn view_repository(&self, db: &Database, repository_id: &RepositoryId) -> Result<PermissionResult, ServerError> {
        let repository = DbRepository::from_id(db, repository_id).await?;
        match repository.status {
            RepositoryStatus::Public | RepositoryStatus::Hidden => {
                return Ok(PermissionResult::Granted);
            }
            _ => {}
        }

        Ok(if let Some(user) = &*self.request_context.connected_user().await {
            if repository.owner == *user.id() {
                PermissionResult::Granted
            } else if Subscription::find(db, user.id(), repository_id).await.is_ok() {
                PermissionResult::Granted
            } else {
                PermissionResult::Denied
            }
        } else {
            PermissionResult::Denied
        })
    }

    pub async fn edit_repository(&self, db: &Database, repository_id: &RepositoryId) -> Result<PermissionResult, ServerError> {
        self.view_repository(db, repository_id).await?.granted();
        let repository = DbRepository::from_id(db, repository_id).await?;
        Ok(if let Some(user) = &*self.request_context.connected_user().await {
            if repository.owner == *user.id() {
                PermissionResult::Granted
            } else if let Ok(subscription) = Subscription::find(db, user.id(), repository_id).await {
                match subscription.access_type {
                    SubscriptionAccessType::Moderator => { PermissionResult::Granted }
                    _ => { PermissionResult::Denied }
                }
            } else {
                PermissionResult::Denied
            }
        } else {
            PermissionResult::Denied
        })
    }

    #[allow(unused)]
    pub async fn upload_to_repository(&self, db: &Database, repository_id: &RepositoryId) -> Result<PermissionResult, ServerError> {
        self.view_repository(db, repository_id).await?.granted();
        let repository = DbRepository::from_id(db, repository_id).await?;
        Ok(if let Some(user) = &*self.request_context.connected_user().await {
            if repository.owner == *user.id() || repository.allow_visitor_upload {
                PermissionResult::Granted
            } else if let Ok(subscription) = Subscription::find(db, user.id(), repository_id).await {
                match subscription.access_type {
                    SubscriptionAccessType::Contributor |
                    SubscriptionAccessType::Moderator => { PermissionResult::Granted }
                    _ => { PermissionResult::Denied }
                }
            } else {
                PermissionResult::Denied
            }
        } else {
            PermissionResult::Denied
        })
    }

    pub async fn view_item(&self, db: &Database, item_id: &ItemId) -> Result<PermissionResult, ServerError> {
        let item = DbItem::from_id(db, item_id, Trash::Both).await?;
        self.view_repository(db, &item.repository).await
    }

    #[allow(unused)]
    pub async fn edit_item(&self, db: &Database, item_id: &ItemId) -> Result<PermissionResult, ServerError> {
        self.view_item(db, item_id).await?.granted();
        let item = DbItem::from_id(db, item_id, Trash::Both).await?;
        if self.edit_repository(db, &item.repository).await?.granted() {
            return Ok(PermissionResult::Granted)
        }
        Ok(if let Some(user) = &*self.request_context.connected_user().await {
            if item.owner == *user.id() {
                PermissionResult::Granted
            } else {
                PermissionResult::Denied
            }
        } else {
            PermissionResult::Denied
        })
    }
}