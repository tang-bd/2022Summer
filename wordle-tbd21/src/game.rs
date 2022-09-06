use super::{stats::*, util::*, *};
use colored::Colorize;
use eframe::egui::{self, vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File};

///Game state indicator
pub enum GameState {
    Continue,
    Won,
    Lost,
    InvalidInput,
    Uninitialized,
}

///Data of a game
#[derive(Deserialize, Serialize, Clone)]
pub struct Game {
    pub answer: String,

    pub guesses: Vec<String>,

    #[serde(skip, default)]
    pub guesses_status: Vec<Vec<char>>,

    #[serde(skip, default)]
    pub letters_status: BTreeMap<char, char>,

    #[serde(skip, default)]
    pub answer_count: BTreeMap<char, usize>,
}

impl Game {
    ///Makes a new Game
    pub fn new(answer: &str) -> Self {
        let answer = answer.to_ascii_uppercase();
        let mut answer_count = BTreeMap::new();
        for letter in answer.chars() {
            answer_count.insert(letter, answer.chars().filter(|c| *c == letter).count());
        }
        Self {
            answer,

            guesses: Vec::new(),

            guesses_status: Vec::new(),

            letters_status: LETTERS.iter().map(|c| (*c, 'X')).collect(),

            answer_count,
        }
    }

    ///Accepts and processes a new guess
    pub fn accept_guess(
        &mut self,
        guess: &str,
        acceptables: &BTreeSet<String>,
        is_difficult: bool,
    ) -> GameState {
        //Preprocess
        let guess = guess.to_ascii_uppercase();
        let mut guess_status = vec!['X'; 5];
        let mut counter = self.answer_count.clone();

        //Guess validation
        if !acceptables.contains(&guess) {
            return GameState::InvalidInput;
        }

        //Judges whether the player's guess is valid when in difficult mode
        if is_difficult {
            for letter in self.answer.chars() {
                if (*self.letters_status.get(&letter).unwrap() == 'G'
                    && guess
                        .chars()
                        .nth(self.answer.chars().position(|c| c == letter).unwrap())
                        .unwrap()
                        != letter)
                    || (*self.letters_status.get(&letter).unwrap() == 'Y'
                        && !guess.contains(letter))
                {
                    return GameState::InvalidInput;
                }
            }
        }

        //Marks all correct letters
        for (j, letter) in guess.chars().enumerate() {
            if self.answer.chars().nth(j).unwrap() == letter {
                self.letters_status.insert(letter, 'G');
                counter.entry(letter).and_modify(|c| {
                    *c -= 1;
                });
                guess_status[j] = 'G';
            }
        }

        //Marks the rest of the letters
        for (j, letter) in guess.chars().enumerate() {
            if guess_status[j] != 'G' {
                //Only scans the letters not marked as green before
                if self.answer.contains(letter) && *counter.get(&letter).unwrap() > 0 {
                    //Marks as yellow
                    guess_status[j] = 'Y';
                    match *self.letters_status.get(&letter).unwrap() {
                        'X' | 'R' => {
                            self.letters_status.insert(letter, 'Y');
                        }
                        _ => (),
                    }
                    counter.entry(letter).and_modify(|c| {
                        *c -= 1;
                    });
                } else {
                    //Marks as red
                    match *self.letters_status.get(&letter).unwrap() {
                        'X' => {
                            self.letters_status.insert(letter, 'R');
                        }
                        _ => (),
                    }
                    guess_status[j] = 'R';
                }
            }
        }

        //Stores the result
        self.guesses.push(guess);
        self.guesses_status.push(guess_status);

        //Decides the game state
        if self.guesses[self.guesses.len() - 1] == self.answer {
            GameState::Won
        } else if self.guesses.len() == 6 {
            GameState::Lost
        } else {
            GameState::Continue
        }
    }

}

///The main struct of the Wordle game application
pub struct Wordle {
    current_game: Game,

    stats: Stats,

    current_guess: String,

    finals: Vec<String>,

    acceptables: BTreeSet<String>,

    game_state: GameState,

    config: Config,

    day: usize,

    stats_filename: String,
}

impl Wordle {
    ///Makes a new Wordle game application from the given configuration
    pub fn new(mut finals: Vec<String>, acceptables: BTreeSet<String>, mut config: Config) -> Self {
        if config.gui {
            //Initialization in GUI mode
            Self {
                current_game: Game::new(random_pick(&finals)),

                stats: Stats::new(),

                current_guess: String::new(),

                finals,

                acceptables,

                game_state: GameState::Uninitialized,

                config,

                day: 0,

                stats_filename: String::new(),
            }
        } else {
            //Initialization in non-GUI mode
            let stats = match config.state {
                Some(ref filename) => match File::open(filename) {
                    Ok(mut file) => {
                        let mut json = String::new();
                        match file.read_to_string(&mut json) {
                            Ok(_) => Stats::from_json(&json)
                                .expect(&format!("{}", "IO failure".red().bold())),
                            Err(_) => Stats::new(),
                        }
                    }
                    Err(_) => Stats::new(),
                },
                None => Stats::new(),
            };

            let day = match config.day {
                Some(d) => d,
                None => 1,
            };

            if config.random {
                //The arguments should not conflict with each other
                if day > finals.len() {
                    invalid_arguments(config.is_tty);
                }

                match config.word {
                    Some(_) => {
                        invalid_arguments(config.is_tty);
                    }
                    None => (),
                }

                finals.shuffle(&mut rand::rngs::StdRng::seed_from_u64(match config.seed {
                    Some(s) => s,
                    None => 0,
                }));
            }

            Self {
                current_game: Game::new(&pick_word(&mut config, &finals, day)),

                stats,

                current_guess: String::new(),

                finals,

                acceptables,

                game_state: GameState::Continue,

                config,

                day,

                stats_filename: String::new(),
            }
        }
    }

    ///Runs the Wordle game application
    pub fn run(self) {
        if self.config.gui {
            self.run_gui();
        } else {
            self.run_no_gui();
        }
    }

    ///Runs the Wordle game application in GUI mode
    fn run_gui(self) {
        let options = eframe::NativeOptions {
            resizable: false,
            initial_window_size: Some(vec2(395.0, 555.0)),
            ..Default::default()
        };

        eframe::run_native("Wordle", options, Box::new(|_cc| Box::new(self)));
    }

    ///Runs the Wordle game application in non-GUI mode
    fn run_no_gui(mut self) {
        //The outer loop -- loop of games
        'outer: loop {
            let mut cguesses_status = vec![];

            //The inner loop -- loop of guesses
            'inner: loop {
                //Reads input and processes the user's guess
                if self.config.is_tty {
                    println!(
                        "Attempt {}:",
                        (self.current_game.guesses.len() + 1).to_string().bold()
                    );
                }
                self.current_guess = String::new();
                io::stdin()
                    .read_line(&mut self.current_guess)
                    .expect(&format!("{}", "IO failure".red().bold()));
                self.current_guess = self.current_guess.trim().to_string().to_ascii_uppercase();

                let state = self.current_game.accept_guess(
                    &self.current_guess,
                    &self.acceptables,
                    self.config.difficult,
                );

                //Handles invalid input
                match state {
                    GameState::InvalidInput => {
                        invalid_input(self.config.is_tty);
                        continue 'inner;
                    }
                    _ => (),
                }

                //Prints result
                if self.config.is_tty {
                    println!("Results:");
                    let mut cguess_status = String::new();
                    let mut cletters_status = String::new();
                    for (i, letter) in self.current_game.guesses_status
                        [self.current_game.guesses.len() - 1]
                        .iter()
                        .enumerate()
                    {
                        cguess_status +=
                            &colorize_tty(*letter, self.current_guess.chars().nth(i).unwrap());
                    }
                    cguesses_status.push(cguess_status);
                    for (i, letter) in self
                        .current_game
                        .letters_status
                        .values()
                        .into_iter()
                        .enumerate()
                    {
                        cletters_status += &colorize_tty(*letter, LETTERS[i]);
                    }
                    for attempt in &cguesses_status {
                        println!("{}", attempt);
                    }
                    println!("{}", cletters_status);

                } else {
                    println!(
                        "{} {}",
                        self.current_game.guesses_status
                            [self.current_game.guesses_status.len() - 1]
                            .iter()
                            .collect::<String>(),
                        self.current_game
                            .letters_status
                            .values()
                            .into_iter()
                            .collect::<String>()
                    );
                }

                //Aftermath
                match state {
                    GameState::Won => {
                        //Prints result
                        if self.config.is_tty {
                            println!(
                                "{}: you attempted {} time{} in total",
                                "Correct".green().bold(),
                                self.current_game.guesses.len().to_string().green().bold(),
                                make_plural(self.current_game.guesses.len() as i32)
                            );
                        } else {
                            println!("CORRECT {}", self.current_game.guesses.len());
                        }

                        //Records game data
                        self.stats.record(self.current_game.clone());
                        if self.config.stats {
                            self.print_stats();
                        }

                        //Asks if the user wants to play one more time
                        if self.config.is_tty {
                            println!("Do you want to play once more? [Y/N]");
                        }
                        if want_to_continue() {
                            self.day += 1;
                            self.current_game =
                                Game::new(&pick_word(&mut self.config, &self.finals, self.day));
                            break 'inner;
                        } else {
                            break 'outer;
                        }
                    }
                    GameState::Lost => {
                        //Prints result
                        if self.config.is_tty {
                            println!(
                                "{}: the answer is {}",
                                "Failed".red().bold(),
                                self.current_game.answer.bright_yellow().bold()
                            );
                        } else {
                            println!("FAILED {}", self.current_game.answer);
                        }

                        //Records game data
                        self.stats.record(self.current_game.clone());
                        if self.config.stats {
                            self.print_stats();
                        }

                        //Asks if the user wants to play one more time
                        if self.config.is_tty {
                            println!("Do you want to play once more? [Y/N]");
                        }
                        if want_to_continue() {
                            self.day += 1;
                            self.current_game =
                                Game::new(&pick_word(&mut self.config, &self.finals, self.day));
                            break 'inner;
                        } else {
                            break 'outer;
                        }
                    }
                    _ => {
                        continue 'inner;
                    }
                }
            }
        }

        //Save the statistics to the given JSON file
        match self.config.state {
            Some(ref filename) => fs::write(filename, self.stats.to_json())
                .expect(&format!("{}", "IO failure".red().bold())),
            None => (),
        }
    }

    ///Prints game statistics
    fn print_stats(&self) {
        if self.config.is_tty {
            println!("{}", "Statistics:".bold());
            println!(
                "You have won {} time{}",
                self.stats.success.to_string().green().bold(),
                make_plural(self.stats.success)
            );
            println!(
                "You have lost {} time{}",
                self.stats.failure.to_string().red().bold(),
                make_plural(self.stats.failure)
            );
            println!(
                "You attempted {} time{} in average to win a game",
                format!("{:.2}", self.stats.average_attempts())
                    .bright_yellow()
                    .bold(),
                make_plural(self.stats.average_attempts().floor() as i32)
            );
            println!("The top 5 words you tried most frequently are:");
            for (word, n) in self.stats.most_frequent() {
                println!(
                    "{}    {} time{}",
                    word.bold(),
                    n.to_string().bold(),
                    make_plural(*n)
                );
            }
        } else {
            println!(
                "{} {} {:.2}",
                self.stats.success,
                self.stats.failure,
                self.stats.average_attempts()
            );
            let mut output = String::new();
            for (word, n) in self.stats.most_frequent() {
                output += &format!("{} {} ", word, n);
            }
            println!("{}", output.trim_end());
        }
    }

    ///Accepts and processes the current guess for GUI mode
    fn accept_current_guess(&mut self) {
        self.game_state = self.current_game.accept_guess(
            &self.current_guess,
            &self.acceptables,
            self.config.difficult,
        );
        self.current_guess = String::new();
    }

    ///Builds a key of the keyboard for GUI mode
    ///Arguments: ch: &char -- the character of the key, ui: &mut egui::Ui -- the UI to build the key on
    fn key(&mut self, ch: &char, ui: &mut egui::Ui) {
        if ui
            .add(
                egui::Button::new(egui::RichText::new(*ch).size(28.0).color(colorize_gui(
                    *self.current_game.letters_status.get(ch).unwrap(),
                )))
                .stroke(egui::Stroke {
                    width: 2.0,
                    color: colorize_gui(*self.current_game.letters_status.get(ch).unwrap()),
                }),
            )
            .clicked()
        {
            self.current_guess.push(*ch);
        }
    }

    ///Builds the bottom panel for the GUI mode
    fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        //Disable the panel if the game hasn't been initialized
        match self.game_state {
            GameState::Uninitialized => {
                ui.set_enabled(false);
            }
            _ => (),
        }

        ui.add_space(5.0);
        egui::Grid::new("keyboard")
            .spacing(vec2(10.0, 10.0))
            .show(ui, |ui| {
                ui.columns(10, |columns| {
                    for (col, ch) in columns.iter_mut().zip(KEYBOARD[..10].iter()) {
                        self.key(ch, col);
                    }
                });

                ui.end_row();

                ui.columns(9, |columns| {
                    for (col, ch) in columns.iter_mut().zip(KEYBOARD[10..19].iter()) {
                        self.key(ch, col);
                    }
                });

                ui.end_row();

                ui.columns(9, |columns| {
                    for (col, ch) in columns.iter_mut().skip(1).zip(KEYBOARD[19..].iter()) {
                        self.key(ch, col);
                    }
                });

                ui.end_row();
            });
        ui.add_space(5.0);
    }

    ///Builds the left panel for the GUI mode
    fn left_panel(&mut self, ui: &mut egui::Ui) {
        //Disable the panel if the game hasn't been initialized
        match self.game_state {
            GameState::Uninitialized => {
                ui.set_enabled(false);
            }
            _ => (),
        }

        //Title
        ui.add_space(5.0);
        ui.label(
            egui::RichText::new("Wordle")
                .size(25.0)
                .color(egui::Color32::WHITE),
        );
        ui.separator();

        //Statistics
        ui.label(
            egui::RichText::new(format!("Played: {}", self.stats.total_rounds))
                .size(20.0)
                .color(egui::Color32::WHITE),
        );
        ui.label(
            egui::RichText::new(format!("Won: {}", self.stats.success))
                .size(20.0)
                .color(egui::Color32::WHITE),
        );
        ui.label(
            egui::RichText::new(format!("Lost: {}", self.stats.failure))
                .size(20.0)
                .color(egui::Color32::WHITE),
        );

        //Input area
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(5.0);
            let response = ui.add_sized(
                vec2(100.0, 30.0),
                egui::TextEdit::singleline(&mut self.current_guess)
                    .hint_text("Your guess")
                    .font(egui::TextStyle::Heading),
            );
            if response.lost_focus() && ui.input().key_pressed(egui::Key::Enter) {
                self.accept_current_guess();
            }
            ui.separator();
        });
    }

    ///Builds the central panel for the GUI mode
    fn central_panel(&mut self, ui: &mut egui::Ui) {
        //Disable the panel if the game hasn't been initialized
        match self.game_state {
            GameState::Uninitialized => {
                ui.set_enabled(false);
            }
            _ => (),
        }

        //Builds the guesses status display area
        egui::Grid::new("guesses")
            .spacing(vec2(10.0, 10.0))
            .show(ui, |ui| {
                for a in 0..6 {
                    for b in 0..5 {
                        let ch = match self.current_game.guesses.get(a) {
                            Some(s) => match s.chars().nth(b) {
                                Some(c) => c,
                                None => ' ',
                            },
                            None => ' ',
                        };
                        if ui
                            .add_sized(
                                vec2(45.0, 60.0),
                                egui::Button::new(
                                    egui::RichText::new(ch)
                                        .size(40.0)
                                        .color(colorize_gui(
                                            self.current_game
                                                .guesses_status
                                                .get(a)
                                                .unwrap_or(&vec!['X'; 5])[b],
                                        ))
                                        .text_style(egui::TextStyle::Heading),
                                )
                                .stroke(egui::Stroke {
                                    width: 2.0,
                                    color: colorize_gui(
                                        self.current_game
                                            .guesses_status
                                            .get(a)
                                            .unwrap_or(&vec!['X'; 5])[b],
                                    ),
                                }),
                            )
                            .clicked()
                        {}
                    }
                    ui.end_row();
                }
            });
    }
}

impl eframe::App for Wordle {
    ///The main function for GUI mode
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        //Builds the panels
        egui::TopBottomPanel::bottom("keyboard")
            .resizable(false)
            .show(&context, |ui| self.bottom_panel(ui));

        egui::SidePanel::left("info")
            .default_width(100.0)
            .resizable(false)
            .show(&context, |ui| self.left_panel(ui));

        egui::CentralPanel::default().show(&context, |ui| self.central_panel(ui));

        //Indicators
        let mut game_over_info_open = true;
        let mut error_info_open = true;
        let mut config_open = true;

        //Reacts to the game state
        match self.game_state {
            GameState::Won => {
                egui::Window::new("Information")
                    .auto_sized()
                    .open(&mut game_over_info_open)
                    .show(context, |ui| {
                        ui.label(
                            egui::RichText::new("You win!")
                                .size(25.0)
                                .color(egui::Color32::WHITE),
                        );
                    });
            }
            GameState::Lost => {
                egui::Window::new("Information")
                    .auto_sized()
                    .open(&mut game_over_info_open)
                    .show(context, |ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "You lose! Answer: {}",
                                self.current_game.answer
                            ))
                            .size(25.0)
                            .color(egui::Color32::WHITE),
                        );
                    });
            }
            GameState::InvalidInput => {
                egui::Window::new("Information")
                    .auto_sized()
                    .open(&mut error_info_open)
                    .show(context, |ui| {
                        ui.label(
                            egui::RichText::new("Invalid input!")
                                .size(25.0)
                                .color(egui::Color32::WHITE),
                        );
                    });
            }
            GameState::Uninitialized => {
                //Initialization on launch
                egui::Window::new("Configuration")
                    .auto_sized()
                    .open(&mut config_open)
                    .show(context, |ui| {
                        ui.checkbox(&mut self.config.difficult, "Difficult mode");
                        if ui
                            .add_sized(
                                vec2(180.0, 20.0),
                                egui::Button::new(
                                    egui::RichText::new("Game data storage file")
                                        .color(egui::Color32::WHITE),
                                ),
                            )
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                self.stats_filename = path.display().to_string();
                            }
                        }
                    });
            }
            GameState::Continue => {}
        }

        //Operations after the windows are closed
        if !game_over_info_open {
            self.game_state = GameState::Continue;
            self.stats.record(self.current_game.clone());
            self.current_game = Game::new(random_pick(&self.finals));
            //Save the statistics to the given JSON file
            if !self.stats_filename.is_empty() {
                fs::write(&self.stats_filename, self.stats.to_json()).unwrap();
            }
        }

        if !error_info_open {
            self.game_state = GameState::Continue;
        }

        if !config_open {
            if !self.stats_filename.is_empty() {
                let mut io_failure = false;
                self.stats = match File::open(&self.stats_filename) {
                    Ok(mut file) => {
                        let mut json = String::new();
                        match file.read_to_string(&mut json) {
                            Ok(_) => match Stats::from_json(&json) {
                                Ok(s) => s,
                                Err(_) => {
                                    io_failure = true;
                                    Stats::new()
                                }
                            },
                            Err(_) => {
                                io_failure = true;
                                Stats::new()
                            }
                        }
                    }
                    Err(_) => {
                        io_failure = true;
                        Stats::new()
                    }
                };

                if io_failure {
                    egui::Window::new("Information")
                        .auto_sized()
                        .open(&mut error_info_open)
                        .show(context, |ui| {
                            ui.label(
                                egui::RichText::new("Invalid game data storage file!")
                                    .color(egui::Color32::WHITE),
                            );
                        });
                }
            }
            self.game_state = GameState::Continue;
        }
    }
}
