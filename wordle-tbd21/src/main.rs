#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod builtin_words;
mod game;
mod stats;
mod util;

use builtin_words::*;
use clap::Parser;
use game::*;
use rand::prelude::*;
use rand::seq::SliceRandom;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use util::*;

//Constants to provide convenience
pub const LETTERS: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

pub const KEYBOARD: [char; 26] = [
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P', 'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L',
    'Z', 'X', 'C', 'V', 'B', 'N', 'M',
];

///Wordle game config
///Can be parsed from either command-line arguments or a JSON file
#[derive(Deserialize, Parser, Clone)]
#[clap(
    author = "TANG Bingda",
    version = "0.1",
    about = "A Wordle game written in Rust",
    long_about = None
    )]
pub struct Config {
    #[serde(default)]
    #[clap(short, long, action)]
    gui: bool,

    #[serde(default)]
    #[clap(short, long, action)]
    random: bool,

    #[serde(default)]
    #[clap(short = 'D', long, action)]
    difficult: bool,

    #[serde(default)]
    #[clap(short = 't', long, action)]
    stats: bool,

    #[serde(default)]
    #[clap(short = 'S', long, action)]
    state: Option<String>,

    #[serde(default)]
    #[clap(short, long, value_parser)]
    word: Option<String>,

    #[serde(default)]
    #[clap(short, long, value_parser)]
    day: Option<usize>,

    #[serde(default)]
    #[clap(short, long, value_parser)]
    seed: Option<u64>,

    #[serde(default)]
    #[clap(short, long = "final-set", value_parser)]
    final_set: Option<String>,

    #[serde(default)]
    #[clap(short, long = "acceptable-set", value_parser)]
    acceptable_set: Option<String>,

    #[serde(skip, default)]
    #[clap(short, long, value_parser)]
    config: Option<String>,

    #[serde(skip, default)]
    #[clap(skip)]
    is_tty: bool,
}

/// The main function for the Wordle game
fn main() -> Result<(), Box<dyn std::error::Error>> {
    //Initializes the configuration of the program from command-line arguments
    let is_tty = atty::is(atty::Stream::Stdout);
    let args = Config {
        is_tty,
        ..Config::parse()
    };
    let config = match args.config {
        Some(filename) => {
            let json: Config = serde_json::from_str(&fs::read_to_string(filename)?)?;
            Config {
                gui: args.gui || json.gui,
                random: args.random || json.random,
                difficult: args.difficult || json.difficult,
                stats: args.stats || json.stats,
                state: match args.state {
                    Some(_) => args.state,
                    None => json.state,
                },
                word: match args.word {
                    Some(_) => args.word,
                    None => json.word,
                },
                day: match args.day {
                    Some(_) => args.day,
                    None => json.day,
                },
                seed: match args.seed {
                    Some(_) => args.seed,
                    None => json.seed,
                },
                final_set: match args.final_set {
                    Some(_) => args.final_set,
                    None => json.final_set,
                },
                acceptable_set: match args.acceptable_set {
                    Some(_) => args.acceptable_set,
                    None => json.acceptable_set,
                },
                config: None,
                is_tty,
            }
        }
        None => args,
    };

    //Initializes wordlists
    let acceptables = match config.acceptable_set {
        Some(ref filename) => {
            let reader = BufReader::new(fs::File::open(filename)?);
            let v = reader
                .lines()
                .map(|s| {
                    let word = s.unwrap().trim().to_ascii_uppercase();
                    if word.len() > 5 {
                        invalid_arguments(is_tty);
                    }
                    word
                })
                .collect::<BTreeSet<_>>();
            v
        }
        None => ACCEPTABLE.iter().map(|s| s.to_ascii_uppercase()).collect(),
    };

    let finals = match config.final_set {
        Some(ref filename) => {
            let reader = BufReader::new(fs::File::open(filename)?);
            let v = reader
                .lines()
                .map(|s| {
                    let word = s.unwrap().trim().to_ascii_uppercase();
                    if word.len() > 5 || !acceptables.contains(&word) {
                        invalid_arguments(is_tty);
                    }
                    word
                })
                .collect::<BTreeSet<_>>();
            v.into_iter().collect::<Vec<_>>()
        }
        None => FINAL.iter().map(|s| s.to_ascii_uppercase()).collect(),
    };

    //Starts Wordle game
    Wordle::new(finals, acceptables, config).run();
    Ok(())
}
