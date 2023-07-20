mod game_manager;

use actix_web::{get, web, App, HttpServer, Responder, post};
use game_manager::GameManager;

use crate::game_manager::fuiz::Fuiz;

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
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
