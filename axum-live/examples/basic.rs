use axum::body::{boxed, Full};
use axum::extract::{FromRequest, FromRequestParts};
use axum::headers::authorization::Bearer;
use axum::headers::Authorization;
use axum::http::request::Parts;
use axum::http::{header, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{async_trait, Extension, Json, Router, Server, TypedHeader};
use jsonwebtoken as jwt;
use jwt::Validation;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

const SECRET_KEY: &[u8] = b"abcdefghijklmnopqrstuvwxy";
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Todo {
    pub id: usize,
    pub user_id: usize,
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateTodo {
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct LoginResponse {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    id: usize,
    name: String,
    exp: usize,
}

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

struct StaticFile<T>(pub T);

impl<T> IntoResponse for StaticFile<T>
where
    T: Into<String>,
{
    fn into_response(self) -> Response {
        let path = self.0.into();
        match Assets::get(path.as_str()) {
            Some(content) => {
                let body = boxed(Full::from(content.data));
                let mime = mime_guess::from_path(path.as_str()).first_or_octet_stream();
                Response::builder()
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .status(StatusCode::OK)
                    .body(body)
                    .unwrap()
            }
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(boxed(Full::from(format!("File not found: {}", path))))
                .unwrap(),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct TodoStore {
    items: Arc<RwLock<Vec<Todo>>>,
}

#[tokio::main]
async fn main() {
    let store = TodoStore {
        items: Arc::new(RwLock::new(vec![Todo {
            id: 0,
            user_id: 0,
            title: "Learn Rust".to_string(),
            completed: false,
        }])),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route(
            "/todos",
            get(todos_handler)
                .post(crate_todo_handler)
                .layer(Extension(store)),
        )
        .route("/login", post(login_handler))
        .fallback(static_handler);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8000));
    println!("Listening on http://{}", addr);

    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn index_handler() -> impl IntoResponse {
    static_handler("/index.html".parse().unwrap()).await
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/').to_string();
    StaticFile(path)
}

async fn todos_handler(
    claims: Claims,
    store: Extension<TodoStore>,
) -> Result<Json<Vec<Todo>>, HttpError> {
    let user_id = claims.id;
    // match store.items.read().await {
    //     Ok(items) => Ok(Json(items
    //         .iter()
    //         .filter(|todo| todo.user_id == user_id)
    //         .map(|todo| todo.clone())
    //         .collect(),
    //     )),
    //     Err(_) => Err(HttpError::Internal),
    // }
    let items = store.items.read().await;
    Ok(Json(
        items
            .iter()
            .filter(|todo| todo.user_id == user_id)
            .map(|todo| todo.clone())
            .collect(),
    ))
}

// Claims 需要实现 FromRequestParts
// Json(todo) 必须放在最后面
async fn crate_todo_handler(
    claims: Claims,
    Extension(store): Extension<TodoStore>,
    Json(todo): Json<CreateTodo>,
) -> Result<StatusCode, HttpError> {
    // match store.items.write().await {
    //     Ok(mut guard) => {
    //         let todo = Todo {
    //             id: get_next_id(),
    //             user_id: claims.id,
    //             title: todo.title,
    //             completed: false,
    //         };
    //         guard.push(todo);
    //         Ok(StatusCode::CREATED)
    //     },
    //     Err(_) => Err(HttpError::Internal)
    // }
    let mut items = store.items.write().await;
    let todo = Todo {
        id: get_next_id(),
        user_id: claims.id,
        title: todo.title,
        completed: false,
    };
    items.push(todo);
    Ok(StatusCode::CREATED)
}

async fn login_handler(Json(_login): Json<LoginRequest>) -> Json<LoginResponse> {
    // skip login info validation
    let claims: Claims = Claims {
        id: 1,
        name: "Abner".to_string(),
        exp: get_epoch() + 14 * 24 * 60 * 60,
    };
    let key = jwt::EncodingKey::from_secret(SECRET_KEY);
    let token = jwt::encode(&jwt::Header::default(), &claims, &key).unwrap();

    Json(LoginResponse { token })
}

#[async_trait]
impl<S, B> FromRequest<S, B> for Claims
where
    // these bounds are required by `async_trait`
    B: Send + 'static,
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request(req: Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request(req, state)
                .await
                .map_err(|e| {
                    println!("FromRequest1: {:?}", e);
                    HttpError::Auth
                })?;

        let key = jwt::DecodingKey::from_secret(SECRET_KEY);
        let token =
            jwt::decode::<Claims>(bearer.token(), &key, &Validation::default()).map_err(|e| {
                println!("FromRequest2: {:?}", e);
                HttpError::Auth
            })?;

        Ok(token.claims)
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|e| {
                    println!("FromRequestParts1: {:?}", e);
                    HttpError::Auth
                })?;

        let key = jwt::DecodingKey::from_secret(SECRET_KEY);
        let token =
            jwt::decode::<Claims>(bearer.token(), &key, &Validation::default()).map_err(|e| {
                println!("FromRequestParts2: {:?}", e);
                HttpError::Auth
            })?;

        Ok(token.claims)
    }
}

#[derive(Debug)]
enum HttpError {
    Auth,
    Internal,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> axum::response::Response {
        let (code, msg) = match self {
            HttpError::Auth => (StatusCode::UNAUTHORIZED, "Unauthorized"),
            HttpError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error"),
        };

        (code, msg).into_response()
    }
}

fn get_epoch() -> usize {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
}

fn get_next_id() -> usize {
    NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
