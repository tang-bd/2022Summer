use super::*;
use colored::Colorize;
use eframe::egui;
use rand::seq::SliceRandom;
use std::io;

///The tool function for adding 's' to plural words
pub fn make_plural(n: i32) -> &'static str {
    if n > 1 {
        "s"
    } else {
        ""
    }
}

///The tool function for colorizing characters according to their status
pub fn colorize_gui(status: char) -> egui::Color32 {
    match status {
        'R' => egui::Color32::LIGHT_RED,
        'Y' => egui::Color32::YELLOW,
        'G' => egui::Color32::LIGHT_GREEN,
        _ => egui::Color32::WHITE,
    }
}

///The tool function for colorizing characters according to their status
///Arguments: status: char -- status indicator, ch: char -- the character to colorize
/// Returns: String -- colorized character
pub fn colorize_tty(status: char, ch: char) -> String {
    match status {
        'G' => format!("{}", String::from(ch).green().bold()),
        'Y' => format!("{}", String::from(ch).bright_yellow().bold()),
        'R' => format!("{}", String::from(ch).red().bold()),
        _ => String::from(ch),
    }
}

///Picks word randomly for GUI mode
pub fn random_pick(finals: &Vec<String>) -> &str {
    finals.choose(&mut rand::thread_rng()).unwrap()
}

///The tool function for printing error information when the arguments are invalid and exiting with a non-zero value
pub fn invalid_arguments(is_tty: bool) {
    if is_tty {
        println!("{}", "Invalid arguments".red().bold());
    }
    std::process::exit(1);
}

///The tool function for printing error information when the player's input are invalid
pub fn invalid_input(is_tty: bool) {
    if is_tty {
        println!("{}", "Invalid input".red().bold());
    } else {
        println!("INVALID");
    }
}

///Asks the player whether to play another time
pub fn want_to_continue() -> bool {
    let mut choice = String::new();
    io::stdin()
        .read_line(&mut choice)
        .expect(&format!("{}", "IO failure".red().bold()));
    match choice.trim().to_ascii_uppercase().as_str() {
        "Y" => true,
        _ => false,
    }
}

///Picks word according to the given configuration for non-GUI mode
pub fn pick_word(config: &mut Config, finals: &Vec<String>, day: usize) -> String {
    match config.random {
        true => finals[day - 1].to_string(),
        false => {
            //The arguments should not conflict with each other
            if config.day.is_some() || config.seed.is_some() {
                invalid_arguments(config.is_tty);
            }
            match config.word {
                Some(ref mut word) => {
                    let result = word.clone();
                    config.word = None;
                    result
                }
                //Reads word from player's input until the input is valid
                None => loop {
                    let mut word = String::new();

                    if config.is_tty {
                        println!("Please enter the answer: ");
                    }

                    io::stdin()
                        .read_line(&mut word)
                        .expect(&format!("{}", "IO failure".red().bold()));
                    word = word.trim().to_string();

                    if finals.contains(&word.to_ascii_uppercase()) {
                        break word;
                    }

                    invalid_input(config.is_tty);
                },
            }
        }
    }
    .to_ascii_uppercase()
}
