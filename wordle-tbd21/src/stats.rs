use super::game::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

///Game statistics storage
#[derive(Deserialize, Serialize)]
pub struct Stats {
    #[serde(skip_deserializing, default)]
    pub total_rounds: i32,

    #[serde(default)]
    pub games: Vec<Game>,

    #[serde(skip, default)]
    pub success: i32,

    #[serde(skip, default)]
    pub failure: i32,

    #[serde(skip, default)]
    pub success_attempts: usize,

    #[serde(skip, default)]
    pub word_counter: BTreeMap<String, i32>,
}

impl Stats {
    ///Makes a new, empty Stats
    pub fn new() -> Self {
        Self {
            total_rounds: 0,
            games: vec![],
            success: 0,
            failure: 0,
            success_attempts: 0,
            word_counter: BTreeMap::new(),
        }
    }

    ///Makes a new Stats from JSON
    pub fn from_json(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut deserialized: Self = serde_json::from_str(json)?;
        deserialized.eval();
        Ok(deserialized)
    }

    ///JSON Serialization
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    ///Scans self.games to evaluate other fields
    pub fn eval(&mut self) {
        for game in &self.games {
            self.total_rounds += 1;
            if game.guesses.len() == 6 && (game.guesses[5] != game.answer) {
                self.failure += 1;
            } else {
                self.success += 1;
                self.success_attempts += game.guesses.len();
            }
            for guess in &game.guesses {
                self.word_counter
                    .entry(guess.clone())
                    .and_modify(|n| *n += 1)
                    .or_insert(1);
            }
        }
    }

    ///Accepts result from a game
    pub fn record(&mut self, game: Game) {
        self.total_rounds += 1;
        if game.guesses.len() == 6 && (game.guesses[5] != game.answer) {
            self.failure += 1;
        } else {
            self.success += 1;
            self.success_attempts += game.guesses.len();
        }
        for guess in &game.guesses {
            self.word_counter
                .entry(guess.clone())
                .and_modify(|n| *n += 1)
                .or_insert(1);
        }
        self.games.push(game);
    }

    ///Calculates the player's average attempts to win a game
    pub fn average_attempts(&self) -> f64 {
        if self.success != 0 {
            self.success_attempts as f64 / self.success as f64
        } else {
            0.0
        }
    }

    ///Returns the top 5 words that the player tried most frequently
    pub fn most_frequent(&self) -> Vec<(&String, &i32)> {
        let mut vec: Vec<_> = self.word_counter.iter().collect();
        vec.sort_by(|&(_, a), &(_, b)| b.cmp(a));
        if vec.len() > 5 {
            vec[..5].to_vec()
        } else {
            vec
        }
    }
}
