mod game_manager;

use crate::game_manager::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    watcher::{WatcherId, WatcherValue},
};
use actix_cors::Cors;
use actix_web::{
    cookie::{Cookie, CookieBuilder},
    get,
    middleware::Logger,
    post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::{session::Session, GameManager};
use std::str::FromStr;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

struct AppState {
    game_manager: GameManager<Session>,
}

#[cfg(debug_assertions)]
fn configure_cookie(cookie: CookieBuilder) -> Cookie {
    cookie
        .same_site(actix_web::cookie::SameSite::Lax)
        .secure(false)
        .path("/")
        .http_only(true)
        .finish()
}

#[cfg(not(debug_assertions))]
fn configure_cookie(cookie: CookieBuilder) -> Cookie {
    cookie
        .same_site(actix_web::cookie::SameSite::None)
        .secure(true)
        .path("/")
        .http_only(true)
        .finish()
}

#[post("/add")]
async fn add(data: web::Data<AppState>, fuiz: web::Json<FuizConfig>) -> impl Responder {
    let game_id = data.game_manager.add_game(fuiz.into_inner());

    let checked_game_id = game_id.clone();

    let host_id = WatcherId::default();

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    ongoing_game.reserve_watcher(host_id, WatcherValue::Host)?;

    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(std::time::Duration::from_secs(60)).await;
            let Some(ongoing_game) = data.game_manager.get_game(&checked_game_id) else {
                break;
            };
            if matches!(
                ongoing_game.state(),
                game_manager::game::GameState::FinalLeaderboard
            ) || ongoing_game.updated().elapsed() > std::time::Duration::from_secs(280)
            {
                ongoing_game.mark_as_done().await;
                data.game_manager.remove_game(&checked_game_id);
                break;
            }
        }
    });

    let cookie = configure_cookie(CookieBuilder::new("wid", host_id.to_string()));

    Ok(HttpResponse::Accepted().cookie(cookie).body(game_id.id))
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

            response.add_cookie(&configure_cookie(CookieBuilder::new(
                "wid",
                watcher_id.to_string(),
            )))?;

            ongoing_game.add_unassigned(watcher_id, own_session).await;

            watcher_id
        }
    };

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = msg_stream.next().await {
            if ongoing_game.state().is_done() {
                break;
            }
            match msg {
                actix_ws::Message::Ping(bytes) => {
                    if session.pong(&bytes).await.is_err() {
                        return;
                    }
                }
                actix_ws::Message::Text(s) => {
                    if let Ok(message) = serde_json::from_str(s.as_ref()) {
                        let inner_game = ongoing_game.clone();
                        actix_web::rt::spawn(async move {
                            inner_game.receive_message(watcher_id, message).await;
                        });
                    }
                }
                _ => break,
            }
        }

        ongoing_game.remove_watcher_session(watcher_id).await;
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
            .wrap(Logger::default())
            .wrap(cors)
            .app_data(app_state.clone())
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(add)
            .service(watch)
    })
    .bind((
        if cfg!(debug_assertions) {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        },
        8080,
    ))?
    .run()
    .await
}
