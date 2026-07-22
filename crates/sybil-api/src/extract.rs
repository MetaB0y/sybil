//! API extractors with one public rejection contract.
//!
//! Axum's built-in extractors intentionally return framework-specific status
//! codes and plain-text bodies. Public REST handlers use these wrappers so
//! path, query, and JSON failures always land as a `400 application/json`
//! [`ApiErrorResponse`](sybil_api_types::ApiErrorResponse).

use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;

use crate::types::error::AppError;

#[derive(Debug)]
pub struct Json<T>(pub T);

impl<T, S> FromRequest<S> for Json<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        axum::Json::<T>::from_request(req, state)
            .await
            .map(|axum::Json(value)| Self(value))
            .map_err(|rejection| AppError::invalid_request("body", rejection.body_text()))
    }
}

impl<T> IntoResponse for Json<T>
where
    axum::Json<T>: IntoResponse,
{
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

#[derive(Debug)]
pub struct Path<T>(pub T);

impl<T, S> FromRequestParts<S> for Path<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Path::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Path(value)| Self(value))
            .map_err(|rejection| AppError::invalid_request("path", rejection.body_text()))
    }
}

#[derive(Debug)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        axum::extract::Query::<T>::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(value)| Self(value))
            .map_err(|rejection| AppError::invalid_request("query", rejection.body_text()))
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use axum::routing::{get, post};
    use http_body_util::BodyExt;
    use serde::Deserialize;
    use sybil_api_types::ApiErrorResponse;
    use tower::ServiceExt;

    use super::{Json, Path, Query};

    #[derive(Deserialize)]
    struct Payload {
        count: u64,
    }

    #[derive(Deserialize)]
    struct Filters {
        limit: u16,
    }

    async fn json(Json(payload): Json<Payload>) -> String {
        payload.count.to_string()
    }

    async fn path(Path(id): Path<u64>) -> String {
        id.to_string()
    }

    async fn query(Query(filters): Query<Filters>) -> String {
        filters.limit.to_string()
    }

    async fn assert_invalid_request(response: axum::response::Response) {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&header::HeaderValue::from_static("application/json"))
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let error: ApiErrorResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error.code, "INVALID_REQUEST");
    }

    #[tokio::test]
    async fn normalizes_json_rejections() {
        let app = Router::new().route("/", post(json));
        let response = app
            .oneshot(
                Request::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"count":"not-an-integer"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_invalid_request(response).await;
    }

    #[tokio::test]
    async fn normalizes_json_unsigned_integer_overflow() {
        let app = Router::new().route("/", post(json));
        let response = app
            .oneshot(
                Request::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"count":18446744073709551616}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_invalid_request(response).await;
    }

    #[tokio::test]
    async fn normalizes_path_rejections() {
        let app = Router::new().route("/{id}", get(path));
        let response = app
            .oneshot(Request::get("/not-an-integer").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_invalid_request(response).await;
    }

    #[tokio::test]
    async fn normalizes_query_rejections() {
        let app = Router::new().route("/", get(query));
        let response = app
            .oneshot(Request::get("/?limit=nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_invalid_request(response).await;
    }
}
