mod game_manager;

use crate::game_manager::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    watcher::{Watcher, WatcherType},
};
use actix_web::{
    cookie::CookieBuilder, get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::{GameManager, session::Session};
use std::{str::FromStr, time::Duration};
use uuid::Uuid;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

struct AppState {
    game_manager: GameManager<Session>,
}

#[get("/dump")]
async fn dump(data: web::Data<AppState>) -> impl Responder {
    format!("{:?}", data.game_manager)
}

#[post("/add")]
async fn add(data: web::Data<AppState>, fuiz: web::Json<FuizConfig>) -> impl Responder {
    let game_id = data.game_manager.add_game(fuiz.into_inner());

    let checked_game_id = game_id.clone();

    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(Duration::from_secs(60)).await;
            let Some(ongoing_game) = data.game_manager.get_game(&checked_game_id) else {
                break;
            };
            if matches!(
                ongoing_game.state(),
                game_manager::game::GameState::FinalLeaderboard
            ) || ongoing_game.updated().elapsed() > Duration::from_secs(280)
            {
                data.game_manager.remove_game(&checked_game_id);
                break;
            }
        }
    });

    format!("{:?}", game_id)
}

#[post("/start/{game_id}")]
async fn start(
    data: web::Data<AppState>,
    game_id: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let game_id = GameId {
        id: game_id.into_inner(),
    };

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    actix_web::rt::spawn(async move { ongoing_game.play().await });

    HttpResponse::Accepted().await
}

#[post("/state/{game_id}")]
async fn state(data: web::Data<AppState>, game_id: web::Path<String>) -> impl Responder {
    let game_id = GameId {
        id: game_id.into_inner(),
    };

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    serde_json::to_string(ongoing_game.state_message().as_ref())
        .map_err(|_| actix_web::error::ErrorInternalServerError("oh no"))
}

#[get("/watch/{game_id}")]
async fn watch(
    req: HttpRequest,
    name: web::Json<Option<String>>,
    data: web::Data<AppState>,
    body: web::Payload,
    game_id: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let (mut response, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;

    let game_id = GameId {
        id: game_id.into_inner(),
    };

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    if ongoing_game.watchers.hosts_count() != 0 && name.is_none() {
        return Err(actix_web::error::ErrorNotFound("Name was not provided"));
    }

    let id = match req.cookie("id") {
        Some(x) => Uuid::from_str(x.value()).unwrap_or(Uuid::new_v4()),
        None => Uuid::new_v4(),
    };

    let own_session = game_manager::session::Session::new(session.clone());

    let watcher = match name.into_inner() {
        Some(n) => Watcher {
            id,
            kind: WatcherType::Player(n),
        },
        None => Watcher {
            id,
            kind: WatcherType::Host,
        },
    };

    ongoing_game
        .add_watcher(watcher.clone(), own_session)
        .await?;

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = msg_stream.next().await {
            match msg {
                actix_ws::Message::Ping(bytes) => {
                    if session.pong(&bytes).await.is_err() {
                        return;
                    }
                }
                actix_ws::Message::Text(s) => {
                    if let Ok(message) = serde_json::from_str(s.as_ref()) {
                        ongoing_game
                            .receive_message(&data.game_manager, watcher.clone(), message)
                            .await;
                    }
                }
                _ => break,
            }
        }

        ongoing_game.remove_watcher(watcher);
        session.close(None).await.ok();
    });

    response.add_cookie(&CookieBuilder::new("id", id.to_string()).finish())?;

    Ok(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();

    let app_state = web::Data::new(AppState {
        game_manager: GameManager::default(),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(dump)
            .service(add)
            .service(watch)
            .service(start)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
