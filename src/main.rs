mod clashmap;
mod clashset;
mod game_manager;

use crate::game_manager::{
    fuiz::config::FuizConfig,
    game::{GameOutgoingMessage, IncomingMessage, OutgoingMessage},
    game_id::GameId,
    session::Tunnel,
    watcher::WatcherId,
    GameVanish,
};
use actix_web::{
    get, middleware::Logger, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::{game::IncomingGhostMessage, session::Session, GameManager};
use itertools::Itertools;
use std::sync::{atomic::AtomicU64, Arc};

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

static_toml::static_toml! {
    #[static_toml(
        suffix = Config,
    )]
    const CONFIG = include_toml!("config.toml");
}

struct AppState {
    game_manager: GameManager<Session>,
}

// fn configure_cookie(cookie: CookieBuilder) -> Cookie {
//     if cfg!(feature = "https") {
//         cookie
//             .same_site(actix_web::cookie::SameSite::None)
//             .secure(true)
//             .path("/")
//             .http_only(true)
//             .finish()
//     } else {
//         cookie
//             .same_site(actix_web::cookie::SameSite::Lax)
//             .secure(false)
//             .path("/")
//             .http_only(true)
//             .finish()
//     }
// }

#[post("/add")]
async fn add(data: web::Data<AppState>, fuiz: web::Json<FuizConfig>) -> impl Responder {
    let game_id = data.game_manager.add_game(fuiz.into_inner());

    let host_id = WatcherId::new();

    data.game_manager.reserve_host(game_id, host_id)?;

    let stale_data = data;

    // Stale Detection
    actix_web::rt::spawn(async move {
        loop {
            actix_web::rt::time::sleep(std::time::Duration::from_secs(60)).await;
            match stale_data.game_manager.alive_check(game_id) {
                Ok(true) => continue,
                Ok(false) => {
                    info!("clearing, {}", game_id);
                    stale_data.game_manager.remove_game(game_id).await;
                }
                _ => break,
            }
        }
    });

    // let cookie = configure_cookie(CookieBuilder::new("wid", host_id.to_string()));

    Ok::<_, GameVanish>(web::Json(serde_json::json!({
        "game_id": game_id,
        "watcher_id": host_id
    })))
}

#[get("/alive/{game_id}")]
async fn alive(data: web::Data<AppState>, game_id: web::Path<GameId>) -> impl Responder {
    data.game_manager
        .exists(game_id.into_inner())
        .is_ok()
        .to_string()
}

#[get("/watch/{game_id}")]
async fn watch(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Payload,
    game_id: web::Path<GameId>,
) -> Result<HttpResponse, actix_web::Error> {
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;

    let game_id = *game_id;

    data.game_manager.exists(game_id)?;

    let own_session = game_manager::session::Session::new(session.clone());

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

    let data_thread = data.clone();

    actix_web::rt::spawn(async move {
        let mut watcher_id = None;
        while let Some(Ok(msg)) = msg_stream.next().await {
            if data.game_manager.exists(game_id).is_err() {
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
                        match message {
                            IncomingMessage::Ghost(IncomingGhostMessage::ClaimId(id))
                                if matches!(
                                    data_thread.game_manager.watcher_exists(game_id, id),
                                    Ok(true)
                                ) =>
                            {
                                if data_thread
                                    .game_manager
                                    .update_session(game_id, id, own_session.clone())
                                    .await
                                    .ok()
                                    .and_then(std::result::Result::ok)
                                    .is_none()
                                {
                                    break;
                                }

                                watcher_id = Some(id);
                            }
                            IncomingMessage::Ghost(_) => {
                                let new_id = WatcherId::new();
                                watcher_id = Some(new_id);

                                if own_session
                                    .send(
                                        &GameOutgoingMessage::IdAssign(new_id)
                                            .to_message()
                                            .expect("default serializer cannot fail"),
                                    )
                                    .await
                                    .is_err()
                                {
                                    break;
                                };

                                if data_thread
                                    .game_manager
                                    .add_unassigned(game_id, new_id, own_session.clone())
                                    .await
                                    .ok()
                                    .and_then(std::result::Result::ok)
                                    .is_none()
                                {
                                    break;
                                };
                            }
                            x => {
                                if let Some(watcher_id) = watcher_id {
                                    let data_thread = data_thread.clone();
                                    actix_web::rt::spawn(async move {
                                        let _ = data_thread
                                            .game_manager
                                            .receive_message(game_id, watcher_id, x)
                                            .await;
                                    });
                                }
                            }
                        }
                    }
                }
                _ => break,
            }
        }

        if let Some(watcher_id) = watcher_id {
            let _ = data
                .game_manager
                .remove_watcher_session(game_id, watcher_id);
        }
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

        #[cfg(feature = "https")]
        {
            let cors = actix_cors::Cors::default()
                .allowed_origin("https://fuiz.us")
                .allowed_methods(vec!["GET", "POST"])
                .allowed_headers(vec![
                    actix_web::http::header::AUTHORIZATION,
                    actix_web::http::header::ACCEPT,
                ])
                .supports_credentials()
                .allowed_header(actix_web::http::header::CONTENT_TYPE);
            app.wrap(cors)
        }
        #[cfg(not(feature = "https"))]
        {
            let cors = actix_cors::Cors::permissive();
            app.wrap(cors)
        }
    })
    .bind((
        if cfg!(feature = "https") {
            "127.0.0.1"
        } else {
            "0.0.0.0"
        },
        8080,
    ))?
    .run()
    .await
}
