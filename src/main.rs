mod game_manager;

use crate::game_manager::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    watcher::{WatcherId, WatcherValue},
};
use actix_cors::Cors;
use actix_web::{
    cookie::CookieBuilder, get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::{session::Session, GameManager};
use std::{str::FromStr, time::Duration};

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

    let host_id = WatcherId::default();

    info!("{:?}", host_id);

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    ongoing_game.reserve_watcher(host_id, WatcherValue::Host)?;

    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(Duration::from_secs(120)).await;
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

    let cookie = CookieBuilder::new("wid", host_id.to_string())
        .same_site(actix_web::cookie::SameSite::None)
        .secure(true)
        .path("/")
        .http_only(true)
        .finish();

    Ok(HttpResponse::Accepted().cookie(cookie).body(game_id.id))
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
    data: web::Data<AppState>,
    req: HttpRequest,
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

    let own_session = game_manager::session::Session::new(session.clone());

    let watcher_id = match req.cookie("wid").map(|x| WatcherId::from_str(x.value())) {
        Some(Ok(watcher_id)) if ongoing_game.has_watcher(watcher_id) => {
            ongoing_game
                .update_session(watcher_id, own_session)
                .await
                .map_err(|_| actix_web::error::ErrorGone("Connection Closed"))?;

            watcher_id
        }
        _ => {
            let watcher_id = WatcherId::default();

            response.add_cookie(
                &CookieBuilder::new("wid", watcher_id.to_string())
                    .same_site(actix_web::cookie::SameSite::None)
                    .secure(true)
                    .http_only(true)
                    .path("/")
                    .finish(),
            )?;

            ongoing_game
                .add_watcher(watcher_id, WatcherValue::Unassigned, own_session)
                .await?;

            watcher_id
        }
    };

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
                        let inner_game = ongoing_game.clone();
                        let inner_data = data.clone();
                        actix_web::rt::spawn(async move {
                            inner_game
                                .receive_message(&inner_data.game_manager, watcher_id, message)
                                .await;
                        });
                    }
                }
                _ => break,
            }
        }

        ongoing_game.remove_watcher_session(watcher_id);
        ongoing_game.announce_waiting().await;
        session.close(None).await.ok();
    });

    Ok(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    pretty_env_logger::init();

    let app_state = web::Data::new(AppState {
        game_manager: GameManager::default(),
    });

    HttpServer::new(move || {
        let cors = Cors::permissive();
        App::new()
            .wrap(cors)
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
