use std::sync::Arc;

use atomig::{Atom, Atomic, Ordering};
use garde::Validate;
use itertools::{izip, Itertools};
use serde::{Deserialize, Serialize};

use crate::{
    clashmap::ClashMap,
    clashset::ClashSet,
    game_manager::{
        game::{Game, IncomingHostMessage, IncomingMessage, IncomingPlayerMessage},
        session::Tunnel,
        watcher::{Id, ValueKind},
    },
};

use super::config::Fuiz;

#[derive(Atom, Clone, Copy, Debug, Default)]
#[repr(u8)]
enum SlideState {
    #[default]
    Unstarted,
    List,
    Winners,
}

const MAX_TEXT_LENGTH: usize = crate::CONFIG.fuiz.answer_text.max_length.unsigned_abs() as usize;
const MAX_ANSWER_COUNT: usize = crate::CONFIG.fuiz.bingo.max_answer_count.unsigned_abs() as usize;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct Slide {
    #[garde(skip)]
    points_awarded: u64,
    #[garde(length(max = MAX_ANSWER_COUNT), inner(length(max = MAX_TEXT_LENGTH)))]
    answers: Vec<String>,
    #[garde(range(max = answers.len()))]
    board_size: usize,

    #[serde(skip)]
    #[garde(skip)]
    user_votes: ClashMap<usize, ClashSet<Id>>,
    #[serde(skip)]
    #[garde(skip)]
    crossed: ClashSet<usize>,
    #[serde(skip)]
    #[garde(skip)]
    state: Arc<Atomic<SlideState>>,
}

#[serde_with::serde_as]
#[derive(Debug, Serialize, Clone)]
pub enum UpdateMessage {
    List {
        index: usize,
        count: usize,
        all_statements: Vec<Word>,
        assigned_statements: Vec<usize>,
        crossed: Vec<usize>,
        user_votes: Vec<usize>,
    },
    Cross {
        crossed: Vec<usize>,
    },
    Votes {
        user_votes: Vec<usize>,
    },
    Winners {
        winners: Vec<String>,
    },
}

#[derive(Debug, Serialize, Clone)]
pub struct Word {
    id: usize,
    text: String,
}

#[serde_with::serde_as]
#[derive(Debug, Serialize, Clone)]
pub enum SyncMessage {
    List {
        index: usize,
        count: usize,
        all_statements: Vec<Word>,
        assigned_statement: Vec<usize>,
        crossed: Vec<usize>,
        user_votes: Vec<usize>,
    },
    Winners {
        index: usize,
        count: usize,
        winners: Vec<String>,
    },
}

fn is_bingo(cells: &[bool]) -> bool {
    let col_count = num_integer::Roots::sqrt(&cells.len());

    let row_count = cells.len() / col_count;

    // check rows
    for row in cells.chunks_exact(col_count) {
        if row.iter().all(|x| *x) {
            return true;
        }
    }

    // check cols
    for col in izip!(cells.chunks(col_count)).filter(|x| x.len() == row_count) {
        if col.iter().all(|x| *x) {
            return true;
        }
    }

    // check diagonals
    if col_count == row_count {
        // upper left to bottom right diagonal
        if col_count * row_count == cells.len()
            && (0..col_count).all(|i| cells.get(i * col_count + i) == Some(&true))
        {
            return true;
        }

        // bottom left to up right diagonal
        if (0..col_count).all(|i| cells.get((row_count - i - 1) * col_count + i) == Some(&true)) {
            return true;
        }
    }

    false
}

impl Slide {
    pub fn play<T: Tunnel>(&self, game: &Game<T>, _fuiz: &Fuiz, index: usize, count: usize) {
        self.send_list(game, index, count);
    }

    fn send_list<T: Tunnel>(&self, game: &Game<T>, index: usize, count: usize) {
        if self.change_state(SlideState::Unstarted, SlideState::List) {
            game.announce_with(|w, v| {
                Some(
                    UpdateMessage::List {
                        index,
                        count,
                        all_statements: self
                            .answers
                            .iter()
                            .enumerate()
                            .map(|(id, s)| Word {
                                id,
                                text: s.to_owned(),
                            })
                            .collect_vec(),
                        assigned_statements: match v {
                            ValueKind::Host | ValueKind::Unassigned => Vec::new(),
                            ValueKind::Player => {
                                let mut rng = fastrand::Rng::new();
                                rng.seed(w.get_seed());
                                rng.choose_multiple(0..self.answers.len(), self.board_size)
                            }
                        },
                        crossed: self.crossed.vec(),
                        user_votes: self.get_user_votes(),
                    }
                    .into(),
                )
            });
        }
    }

    fn change_state(&self, before: SlideState, after: SlideState) -> bool {
        self.state
            .compare_exchange(before, after, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn state(&self) -> SlideState {
        self.state.load(Ordering::SeqCst)
    }

    fn get_winners<T: Tunnel>(&self, game: &Game<T>) -> Vec<String> {
        self.get_winners_id(game)
            .into_iter()
            .filter_map(|x| game.get_name(x))
            .collect_vec()
    }

    fn get_winners_id<T: Tunnel>(&self, game: &Game<T>) -> Vec<Id> {
        game.players()
            .iter()
            .filter_map(|w| {
                let mut rng = fastrand::Rng::new();
                rng.seed(w.get_seed());
                let players_board = rng.choose_multiple(0..self.answers.len(), self.board_size);

                let bingo_board = players_board
                    .iter()
                    .map(|x| self.crossed.contains(x))
                    .collect_vec();

                if is_bingo(&bingo_board) {
                    Some(*w)
                } else {
                    None
                }
            })
            .collect_vec()
    }

    fn send_winners<T: Tunnel>(&self, game: &Game<T>) {
        if self.change_state(SlideState::List, SlideState::Winners) {
            game.announce(
                &UpdateMessage::Winners {
                    winners: self.get_winners(game),
                }
                .into(),
            );
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        watcher_id: Id,
        watcher_kind: ValueKind,
        game: &Game<T>,
        index: usize,
        count: usize,
    ) -> SyncMessage {
        match self.state() {
            SlideState::Unstarted | SlideState::List => SyncMessage::List {
                index,
                count,
                all_statements: self
                    .answers
                    .iter()
                    .enumerate()
                    .map(|(id, s)| Word {
                        id,
                        text: s.to_owned(),
                    })
                    .collect_vec(),
                assigned_statement: match watcher_kind {
                    ValueKind::Host | ValueKind::Unassigned => Vec::new(),
                    ValueKind::Player => {
                        let mut rng = fastrand::Rng::new();
                        rng.seed(watcher_id.get_seed());
                        rng.choose_multiple(0..self.answers.len(), self.board_size)
                    }
                },
                crossed: self.crossed.vec(),
                user_votes: self.get_user_votes(),
            },
            SlideState::Winners => SyncMessage::Winners {
                index,
                count,
                winners: self.get_winners(game),
            },
        }
    }

    fn get_user_votes(&self) -> Vec<usize> {
        let mut user_votes = vec![0; self.answers.len()];
        for (i, users) in self.user_votes.clone().vec() {
            if let Some(u) = user_votes.get_mut(i) {
                *u = users.len();
            }
        }
        user_votes
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        _fuiz: &Fuiz,
        watcher_id: Id,
        message: &IncomingMessage,
        index: usize,
        count: usize,
    ) {
        match message {
            IncomingMessage::Host(host_message) => match host_message {
                IncomingHostMessage::Next => match self.state.load(Ordering::SeqCst) {
                    SlideState::Unstarted => self.send_list(game, index, count),
                    SlideState::List => self.send_winners(game),
                    SlideState::Winners => {
                        game.leaderboard.add_scores(
                            &self
                                .get_winners_id(game)
                                .into_iter()
                                .map(|i| (i, self.points_awarded))
                                .into_grouping_map_by(|(id, _)| game.leaderboard_id(*id))
                                .max_by_key(|_, (_, score)| *score)
                                .into_iter()
                                .map(|(id, (_, score))| (id, score))
                                .chain(game.players().into_iter().map(|id| (id, 0)))
                                .unique_by(|(id, _)| *id)
                                .collect_vec(),
                        );
                        game.finish_slide().await;
                    }
                },
                IncomingHostMessage::Index(u) => {
                    self.crossed.insert(*u);
                    let winners = self.get_winners(game);
                    game.announce(
                        &UpdateMessage::Cross {
                            crossed: self.crossed.vec(),
                        }
                        .into(),
                    );
                    if !winners.is_empty() {
                        self.send_winners(game);
                    }
                }
                IncomingHostMessage::Lock(_) => {}
            },
            IncomingMessage::Player(IncomingPlayerMessage::IndexAnswer(v)) => {
                self.user_votes.modify_entry_or_default(*v, |s| {
                    s.insert(watcher_id);
                });
                game.announce(
                    &UpdateMessage::Votes {
                        user_votes: self.get_user_votes(),
                    }
                    .into(),
                );
                if !self.get_winners_id(game).is_empty() {
                    self.send_winners(game);
                }
            }
            _ => (),
        }
    }
}
