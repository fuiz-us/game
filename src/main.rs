mod game_manager;

use crate::game_manager::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    watcher::{WatcherId, WatcherValue},
};
use actix_web::{
    cookie::{Cookie, CookieBuilder},
    get,
    middleware::Logger,
    post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::{session::Session, GameManager};
use itertools::Itertools;
use std::{
    str::FromStr,
    sync::{atomic::AtomicU64, Arc},
};

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

    let stale_game_id = game_id.clone();

    let host_id = WatcherId::default();

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    ongoing_game.reserve_watcher(host_id, WatcherValue::Host)?;

    let stale_data = data;

    // Stale Detection
    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(std::time::Duration::from_secs(60)).await;
            let Some(ongoing_game) = stale_data.game_manager.get_game(&stale_game_id) else {
                break;
            };
            if matches!(ongoing_game.state(), game_manager::game::GameState::Done)
                || ongoing_game.updated().elapsed() > std::time::Duration::from_secs(280)
            {
                ongoing_game.mark_as_done().await;
                stale_data.game_manager.remove_game(&stale_game_id);
                break;
            }
        }
    });

    let cookie = configure_cookie(CookieBuilder::new("wid", host_id.to_string()));

    Ok(HttpResponse::Ok().cookie(cookie).body(game_id.id))
}

#[get("/alive/{game_id}")]
async fn alive(data: web::Data<AppState>, game_id: web::Path<String>) -> impl Responder {
    match data.game_manager.get_game(&GameId {
        id: game_id.into_inner().to_uppercase(),
    }) {
        Some(x) => !x.state().is_done(),
        None => false,
    }
    .to_string()
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
        id: game_id.into_inner().to_uppercase(),
    };

    let Some(ongoing_game) = data.game_manager.get_game(&game_id) else {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    };

    if ongoing_game.state().is_done() {
        return Err(actix_web::error::ErrorNotFound("GameId not found"));
    }

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

    let mut heartbeat_session = session.clone();

    let latest_value = Arc::new(AtomicU64::new(0));

    let sender_latest_value = latest_value.clone();
    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(std::time::Duration::from_secs(5)).await;
            let new_value = fastrand::u64(0..u64::MAX);
            sender_latest_value.store(new_value, atomig::Ordering::SeqCst);
            if heartbeat_session
                .ping(&new_value.to_ne_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    actix_web::rt::spawn(async move {
        while let Some(Ok(msg)) = msg_stream.next().await {
            if ongoing_game.state().is_done() {
                break;
            }
            match msg {
                actix_ws::Message::Pong(bytes) => {
                    let last_value = latest_value.load(atomig::Ordering::SeqCst);
                    if let Ok(actual_bytes) = bytes.into_iter().collect_vec().try_into() {
                        let value = u64::from_ne_bytes(actual_bytes);
                        if last_value != value {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                actix_ws::Message::Ping(bytes) => {
                    if session.pong(&bytes).await.is_err() {
                        break;
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
        let app = App::new()
            .wrap(Logger::default())
            .app_data(app_state.clone())
            .route("/hello", web::get().to(|| async { "Hello World!" }))
            .service(alive)
            .service(add)
            .service(watch);

        #[cfg(debug_assertions)]
        {
            let cors = actix_cors::Cors::permissive();
            app.wrap(cors)
        }
        #[cfg(not(debug_assertions))]
        {
            app
        }
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
