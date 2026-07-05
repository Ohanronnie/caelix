use std::sync::Arc;

use caelix::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool, sqlite::SqlitePoolOptions};

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn connect(database_url: &str) -> std::result::Result<Self, sqlx::Error> {
        let max_connections = if database_url.starts_with("sqlite::memory:") {
            1
        } else {
            5
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                email TEXT NOT NULL UNIQUE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

pub async fn connect_database(
    _container: Arc<Container>,
) -> std::result::Result<Database, sqlx::Error> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://examples/sqlx-crud/users.db?mode=rwc".to_string());

    Database::connect(&database_url).await
}

#[derive(Debug, Clone, Serialize, FromRow, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserDto {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserDto {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[injectable]
pub struct UserRepository {
    database: Arc<Database>,
}

impl UserRepository {
    pub async fn find_all(&self) -> std::result::Result<Vec<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users ORDER BY id")
            .fetch_all(self.database.pool())
            .await
    }

    pub async fn find_by_id(&self, id: i64) -> std::result::Result<Option<User>, sqlx::Error> {
        sqlx::query_as::<_, User>("SELECT id, name, email FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(self.database.pool())
            .await
    }

    pub async fn create(&self, body: CreateUserDto) -> std::result::Result<User, sqlx::Error> {
        let result = sqlx::query("INSERT INTO users (name, email) VALUES (?, ?)")
            .bind(body.name)
            .bind(body.email)
            .execute(self.database.pool())
            .await?;

        let id = result.last_insert_rowid();

        self.find_by_id(id).await?.ok_or(sqlx::Error::RowNotFound)
    }

    pub async fn update(
        &self,
        id: i64,
        body: UpdateUserDto,
    ) -> std::result::Result<Option<User>, sqlx::Error> {
        let result = sqlx::query(
            r#"
            UPDATE users
            SET
                name = COALESCE(?, name),
                email = COALESCE(?, email)
            WHERE id = ?
            "#,
        )
        .bind(body.name)
        .bind(body.email)
        .bind(id)
        .execute(self.database.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.find_by_id(id).await
    }

    pub async fn delete(&self, id: i64) -> std::result::Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(self.database.pool())
            .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[injectable]
pub struct UserService {
    repository: Arc<UserRepository>,
}

impl UserService {
    pub async fn list_users(&self) -> Result<Response<Vec<User>>> {
        Ok(Response::Body(self.repository.find_all().await?))
    }

    pub async fn get_user(&self, id: i64) -> Result<Response<User>> {
        let user = self
            .repository
            .find_by_id(id)
            .await?
            .ok_or_else(|| NotFoundException::new(format!("User {id} was not found")))?;

        Ok(Response::Body(user))
    }

    pub async fn create_user(&self, body: CreateUserDto) -> Result<Response<User>> {
        let user = self.repository.create(body).await?;
        Ok(Response::WithStatus(StatusCode::CREATED, user))
    }

    pub async fn update_user(&self, id: i64, body: UpdateUserDto) -> Result<Response<User>> {
        let user = self
            .repository
            .update(id, body)
            .await?
            .ok_or_else(|| NotFoundException::new(format!("User {id} was not found")))?;

        Ok(Response::Body(user))
    }

    pub async fn delete_user(&self, id: i64) -> Result<Response<()>> {
        if self.repository.delete(id).await? {
            Ok(Response::no_content())
        } else {
            Err(NotFoundException::new(format!("User {id} was not found")))
        }
    }
}

#[injectable]
pub struct UserController {
    service: Arc<UserService>,
}

#[controller("/users")]
impl UserController {
    #[get("")]
    pub async fn list_users(&self) -> Result<Response<Vec<User>>> {
        self.service.list_users().await
    }

    #[get("/{id}")]
    pub async fn get_user(&self, #[param] id: i64) -> Result<Response<User>> {
        self.service.get_user(id).await
    }

    #[post("")]
    pub async fn create_user(&self, #[body] body: CreateUserDto) -> Result<Response<User>> {
        self.service.create_user(body).await
    }

    #[patch("/{id}")]
    pub async fn update_user(
        &self,
        #[param] id: i64,
        #[body] body: UpdateUserDto,
    ) -> Result<Response<User>> {
        self.service.update_user(id, body).await
    }

    #[delete("/{id}")]
    pub async fn delete_user(&self, #[param] id: i64) -> Result<Response<()>> {
        self.service.delete_user(id).await
    }
}

pub struct UserModule;

impl Module for UserModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<Database, _, _>(connect_database)
            .provider::<UserRepository>()
            .provider::<UserService>()
            .controller::<UserController>()
    }
}

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UserModule>()
    }
}
