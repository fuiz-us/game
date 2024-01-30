# Fuiz

Host live quizzes freely

<img src="https://gitlab.com/opencode-mit/fuiz-website/-/raw/main/static/favicon.svg?ref_type=heads" width="128" height="128" alt="Switcheroo icon">

[![License](https://img.shields.io/gitlab/license/opencode-mit/fuiz?style=for-the-badge)](https://gitlab.com/opencode-mit/fuiz/-/raw/main/LICENSE)

## Developing

This is the backend component. It can be run with:

```
cargo run
```

This will open a server listening to port 8080.

### Creating a game

```http
POST /add
```

| Parameter | Type          | Description                                                                                               |
| :-------- | :------------ | :-------------------------------------------------------------------------------------------------------- |
| `config`  | `FuizConfig`  | **Required**. Config as defined in [src/game_manager/fuiz/config.rs](src/game_manager/fuiz/config.rs#L31) |
| `options` | `FuizOptions` | **Required**. Options as defined in [src/game_manager/game.rs](src/game_manager/game.rs#L41)              |

#### Response

```javascript
{
  "game_id"    : string,
  "watcher_id" : string
}
```

### Checking if game is alive

```http
GET /alive/:gameid
```

#### Response

```javascript
true | false;
```

### Joining a game (using WS protocol)

```http
GET /watch/:gameid
```

This establishes a websocket connection. #TODO: documenting websocket messages.

## Status

While you could host this yourself, a live version exists on [api.fuiz.us](https://api.fuiz.us). Its status can be checked on: [status.fuiz.us](https://status.fuiz.us).
