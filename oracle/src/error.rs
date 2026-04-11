//! Top-level error type. Library functions return their own narrower errors;
//! HTTP handlers map them all to [`ApiError`] which serialises as
//! `application/problem+json`.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    BadRequest(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("not yet implemented: {0}")]
    NotImplemented(&'static str),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("slicer error: {0}")]
    Slicer(#[from] crate::slicer::SlicerError),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            Self::Internal(_) | Self::Db(_) | Self::Slicer(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn type_uri(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "/v1/errors/not-found",
            Self::BadRequest(_) => "/v1/errors/bad-request",
            Self::Conflict(_) => "/v1/errors/conflict",
            Self::PreconditionFailed(_) => "/v1/errors/precondition-failed",
            Self::NotImplemented(_) => "/v1/errors/not-implemented",
            Self::Internal(_) => "/v1/errors/internal",
            Self::Db(_) => "/v1/errors/db",
            Self::Slicer(_) => "/v1/errors/slicer",
        }
    }
}

#[derive(Debug, Serialize)]
struct ProblemJson {
    #[serde(rename = "type")]
    type_uri: &'static str,
    title: String,
    status: u16,
    detail: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ProblemJson {
            type_uri: self.type_uri(),
            title: status.canonical_reason().unwrap_or("error").to_string(),
            status: status.as_u16(),
            detail: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
