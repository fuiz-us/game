mod game_manager;

use std::str::FromStr;

use actix_web::{
    cookie::CookieBuilder, get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use futures_util::StreamExt;
use game_manager::GameManager;
use uuid::Uuid;

use crate::game_manager::{fuiz::Fuiz, game::GameId};

struct AppState {
    game_manager: GameManager,
}

#[get("/dump")]
async fn dump(data: web::Data<AppState>) -> impl Responder {
    format!("{:?}", data.game_manager)
}

#[post("/add")]
async fn add(data: web::Data<AppState>, fuiz: web::Json<Fuiz>) -> impl Responder {
    data.game_manager.add_game(fuiz.into_inner());
    format!("{:?}", data.game_manager)
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

    actix_web::rt::spawn(async move { ongoing_game.start().await });

    HttpResponse::Accepted().await
}

#[get("/watch/{game_id}")]
async fn watch(
    req: HttpRequest,
    data: web::Data<AppState>,
    body: web::Payload,
    game_id: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let (mut response, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;

    let game_id = GameId {
        id: game_id.into_inner(),
    };

    if let Some(ongoing_game) = data.game_manager.get_game(&game_id) {
        let id = match req.cookie("id") {
            Some(x) => Uuid::from_str(x.value()).unwrap_or(Uuid::new_v4()),
            None => Uuid::new_v4(),
        };

        actix_web::rt::spawn(async move {
            let own_session = game_manager::session::Session::new(session.clone());
            ongoing_game.add_listener(id, own_session);

            while let Some(Ok(msg)) = msg_stream.next().await {
                match msg {
                    actix_ws::Message::Ping(bytes) => {
                        if session.pong(&bytes).await.is_err() {
                            return;
                        }
                    }
                    actix_ws::Message::Text(s) => {
                        if let Ok(message) = serde_json::from_str(s.as_ref()) {
                            ongoing_game.receive_message(id, message).await;
                        }
                    }
                    _ => break,
                }
            }

            ongoing_game.remove_listener(id);
            session.close(None).await.ok();
        });

        response.add_cookie(&CookieBuilder::new("id", id.to_string()).finish())?;

        Ok(response)
    } else {
        Err(actix_web::error::ErrorNotFound("GameId not found"))
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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
