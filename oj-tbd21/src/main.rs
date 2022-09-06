mod database;

use actix_web::{
    get, middleware::Logger, post, put, web, App, HttpResponse, HttpServer, Responder,
};
use chrono::{DateTime, Utc};
use clap::Parser;
use database::*;
use date_time_format::*;
use env_logger;
use log;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    error::Error,
    fs,
    io::{self, Write},
    ops::{Deref, DerefMut},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

///Tool macro to simplify error handling
macro_rules! oj_try {
    ($x:expr) => {
        match $x {
            Ok(v) => v,
            Err(e) => {
                return internal_error(e);
            }
        }
    };
}

///Module for formatting DateTime<Utc>
mod date_time_format {
    use chrono::{DateTime, TimeZone, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub const FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = format!("{}", date.format(FORMAT));
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'d, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'d>,
    {
        let s = String::deserialize(deserializer)?;
        Utc.datetime_from_str(&s, FORMAT)
            .map_err(serde::de::Error::custom)
    }
}

//Default server configurations

pub fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

pub fn default_bind_port() -> u16 {
    12345
}

///Server configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Server {
    #[serde(default = "default_bind_address")]
    bind_address: String,

    #[serde(default = "default_bind_port")]
    bind_port: u16,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ProblemType {
    Standard,
    Strict,
    Spj,
    DynamicRanking,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug)]
pub enum OjState {
    Queueing,
    Running,
    Finished,
    Canceled,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug)]
pub enum OjResult {
    Waiting,
    Running,
    Accepted,

    #[serde(rename = "Compilation Error")]
    CompilationError,

    #[serde(rename = "Compilation Success")]
    CompilationSuccess,

    #[serde(rename = "Wrong Answer")]
    WrongAnswer,

    #[serde(rename = "Runtime Error")]
    RuntimeError,

    #[serde(rename = "Time Limit Exceeded")]
    TimeLimitExceeded,

    #[serde(rename = "Memory Limit Exceeded")]
    MemoryLimitExceeded,

    #[serde(rename = "System Error")]
    SystemError,

    #[serde(rename = "SPJ Error")]
    SpjError,
    Skipped,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Case {
    score: f32,
    input_file: String,
    answer_file: String,
    time_limit: u64,
    memory_limit: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct CaseResult {
    id: usize,
    result: OjResult,
    time: u128,
    memory: u128,
    info: String,
}

///Miscellaneous configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Misc {
    special_judge: Option<Vec<String>>,
    dynamic_ranking_ratio: Option<f32>,
}

///Problem configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Problem {
    id: usize,
    name: String,

    #[serde(rename = "type")]
    problem_type: ProblemType,
    misc: Misc,
    cases: Vec<Case>,
}

///Language configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Language {
    name: String,
    file_name: String,
    command: Vec<String>,
}

///Overall configuration
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    server: Server,
    problems: Vec<Problem>,
    languages: Vec<Language>,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorReason {
    ErrInvalidArgument,
    ErrInvalidState,
    ErrNotFound,
    ErrRateLimit,
    ErrExternal,
    ErrInternal,
}

///Body of response when errors occur
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ErrorResponseBody {
    code: u16,
    reason: ErrorReason,
    message: String,
}

///Shortcut to generate response for internal error
fn internal_error(e: Box<dyn Error>) -> HttpResponse {
    HttpResponse::InternalServerError().body(
        serde_json::to_string(&ErrorResponseBody {
            code: 6,
            reason: ErrorReason::ErrInternal,
            message: format!("Internal error: {}", e.to_string()),
        })
        .unwrap(),
    )
}

///Body of submission response
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Submission {
    source_code: String,
    language: String,
    user_id: usize,
    contest_id: usize,
    problem_id: usize,
}

///Information, configuration and result of a job
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Job {
    id: usize,
    created_time: UtcDateTime,
    updated_time: UtcDateTime,

    submission: Submission,
    state: OjState,
    result: OjResult,
    score: f32,
    cases: Vec<CaseResult>,
}

///Information and configuration of a contest
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Contest {
    id: Option<usize>,
    name: String,
    from: UtcDateTime,
    to: UtcDateTime,
    problem_ids: Vec<usize>,
    user_ids: Vec<usize>,
    submission_limit: usize,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct User {
    #[serde(default)]
    id: Option<usize>,
    name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct UsersRanking {
    user: User,
    rank: usize,
    scores: Vec<f32>,

    #[serde(skip_serializing)]
    max_time: UtcDateTime,

    #[serde(skip)]
    submission_count: usize,
}

///Job filter
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Filter {
    user_id: Option<usize>,
    user_name: Option<String>,
    contest_id: Option<usize>,
    problem_id: Option<usize>,
    language: Option<String>,
    from: Option<UtcDateTime>,
    to: Option<UtcDateTime>,
    state: Option<OjState>,
    result: Option<OjResult>,
}

impl Filter {
    ///Applies the filter to the SQLite database to get desired jobs
    fn apply(&self, pool: &Pool<SqliteConnectionManager>) -> Result<Vec<Job>, Box<dyn Error>> {
        //Records whether error occurs when selecting a user by name
        let mut error = None;

        //Do filtering
        let filtered = Job::select_all(&pool)?
            .into_iter()
            .filter(|job| {
                let mut ok = true;

                match &self.user_name {
                    Some(name) => match User::select_by_name(name, &pool) {
                        Ok(Some(user)) => {
                            if job.submission.user_id != user.id.unwrap() {
                                ok = false;
                            }
                        }
                        Ok(None) => {}
                        Err(e) => error = Some(e),
                    },
                    None => {}
                }

                match self.user_id {
                    Some(user_id) => {
                        if job.submission.user_id != user_id {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.contest_id {
                    Some(contest_id) => {
                        if job.submission.contest_id != contest_id && job.submission.contest_id != 0
                        {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.problem_id {
                    Some(problem_id) => {
                        if job.submission.problem_id != problem_id {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.language {
                    Some(ref language) => {
                        if &job.submission.language != language {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.user_id {
                    Some(user_id) => {
                        if job.submission.user_id != user_id {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.state {
                    Some(state) => {
                        if job.state != state {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.result {
                    Some(result) => {
                        if job.result != result {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.from {
                    Some(from) => {
                        if *job.created_time < *from {
                            ok = false;
                        }
                    }
                    None => {}
                }

                match self.to {
                    Some(to) => {
                        if *job.created_time > *to {
                            ok = false;
                        }
                    }
                    None => {}
                }
                ok
            })
            .collect::<Vec<_>>();

        //Checks whether error occurred
        match error {
            Some(e) => Err(e),
            None => Ok(filtered),
        }
    }
}

///Wrapped DateTime<Utc> for the convenience of serialization and deserialization
#[derive(Deserialize, Serialize, Clone, Copy, Debug)]
#[serde(transparent)]
pub struct UtcDateTime {
    #[serde(with = "date_time_format")]
    time: DateTime<Utc>,
}

//Simplifies operations on the inner DateTime<Utc>

impl Deref for UtcDateTime {
    type Target = DateTime<Utc>;

    fn deref(&self) -> &Self::Target {
        &self.time
    }
}

impl DerefMut for UtcDateTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.time
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ScoringRule {
    Latest,
    Highest,
}

#[derive(Deserialize, Serialize, Clone, Debug, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TieBreaker {
    SubmissionTime,
    SubmissionCount,
    UserId,
}

#[derive(Deserialize, Serialize, Clone, Debug, Copy)]
pub struct RankingRule {
    scoring_rule: Option<ScoringRule>,
    tie_breaker: Option<TieBreaker>,
}

///Command-line arguments
#[derive(Parser)]
#[clap(
    author = "TANG Bingda",
    version = "0.1",
    about = "Online Judge",
    long_about = None
    )]
pub struct Cli {
    #[clap(short, long, value_parser)]
    config: Option<String>,

    #[clap(short, long = "flush-data", action)]
    flush_data: bool,
}

///POST requests for "/jobs" handler
#[post("/jobs")]
async fn post_jobs(
    submission: web::Json<Submission>,
    config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    log::info!(target: "post_jobs_handler", "Handling POST for problem {} in contest {}", submission.problem_id, submission.contest_id);

    let created_time = UtcDateTime { time: Utc::now() };

    //Unwraps the arguments
    let submission = submission.into_inner();
    let pool = pool.into_inner();
    let config = config.into_inner();

    //Checks the request

    if !config
        .languages
        .iter()
        .map(|lang| &lang.name)
        .collect::<Vec<_>>()
        .contains(&&submission.language)
    {
        return HttpResponse::NotFound().body(
            serde_json::to_string(&ErrorResponseBody {
                code: 3,
                reason: ErrorReason::ErrNotFound,
                message: format!("Language {} not supported.", submission.language),
            })
            .unwrap(),
        );
    }

    if !config
        .problems
        .iter()
        .map(|problem| problem.id)
        .collect::<Vec<_>>()
        .contains(&submission.problem_id)
    {
        return HttpResponse::NotFound().body(
            serde_json::to_string(&ErrorResponseBody {
                code: 3,
                reason: ErrorReason::ErrNotFound,
                message: format!("Problem {} not found.", submission.problem_id),
            })
            .unwrap(),
        );
    }

    //Contest-related checks
    match oj_try!(Contest::select_by_id(submission.contest_id, &pool)) {
        Some(contest) => {
            if !contest.problem_ids.contains(&submission.problem_id) {
                return HttpResponse::BadRequest().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 1,
                        reason: ErrorReason::ErrInvalidArgument,
                        message: format!(
                            "Contest {} does not contains problem {}.",
                            contest.id.unwrap(),
                            submission.problem_id
                        ),
                    })
                    .unwrap(),
                );
            }
            if !contest.user_ids.contains(&submission.user_id) {
                return HttpResponse::BadRequest().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 1,
                        reason: ErrorReason::ErrInvalidArgument,
                        message: format!(
                            "Contest {} does not contains user {}.",
                            contest.id.unwrap(),
                            submission.user_id
                        ),
                    })
                    .unwrap(),
                );
            }
            if *created_time < *contest.from || *created_time > *contest.to {
                return HttpResponse::BadRequest().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 1,
                        reason: ErrorReason::ErrInvalidArgument,
                        message: format!("Contest {} is not open now", contest.id.unwrap()),
                    })
                    .unwrap(),
                );
            }
            if {
                oj_try!(Filter {
                    user_id: Some(submission.user_id),
                    contest_id: Some(contest.id.unwrap()),
                    problem_id: Some(submission.problem_id),
                    ..Default::default()
                }
                .apply(&pool))
                .len()
                    == contest.submission_limit
            } {
                return HttpResponse::BadRequest().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 4,
                        reason: ErrorReason::ErrRateLimit,
                        message: format!("Submission limit reached"),
                    })
                    .unwrap(),
                );
            }
        }
        None => {
            if submission.contest_id != 0 {
                return HttpResponse::NotFound().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 3,
                        reason: ErrorReason::ErrNotFound,
                        message: format!("Contest {} not found.", submission.contest_id),
                    })
                    .unwrap(),
                );
            }
        }
    }

    match oj_try!(User::select_by_id(submission.user_id, &pool)) {
        Some(_) => {}
        None => {
            return HttpResponse::NotFound().body(
                serde_json::to_string(&ErrorResponseBody {
                    code: 3,
                    reason: ErrorReason::ErrNotFound,
                    message: format!("User {} not found.", submission.user_id),
                })
                .unwrap(),
            );
        }
    }

    //Does judging
    let job = oj_try!(judge(
        oj_try!(Job::count(&pool)),
        &submission,
        config.clone(),
        created_time,
        created_time,
    ));

    //Stores to the SQLite database
    oj_try!(job.insert(&pool));

    HttpResponse::Ok().body(serde_json::to_string(&job).unwrap())
}

///Judges the submission and create a new Job record
fn judge(
    id: usize,
    submission: &Submission,
    config: Arc<Config>,
    created_time: UtcDateTime,
    updated_time: UtcDateTime,
) -> Result<Job, Box<dyn Error>> {
    //Initializes required variables
    let mut score = 0.0;
    let mut result = OjResult::Accepted;
    let language = config
        .languages
        .iter()
        .filter(|language| language.name == submission.language)
        .next()
        .unwrap();
    let problem = config
        .problems
        .iter()
        .find(|problem| problem.id == submission.problem_id)
        .unwrap();
    let mut case_results = vec![];

    //Prepare the file system ready for the following steps
    let temp_dir = format!("temp/{}", created_time.format(FORMAT).to_string());
    fs::create_dir_all(&temp_dir)?;
    let mut source_code = fs::File::create(format!("{}/{}", temp_dir, language.file_name))?;
    source_code.write_all(submission.source_code.as_bytes())?;

    //Compilation arguments preparation
    let args = &language
        .command
        .iter()
        .map(|arg| match arg.as_str() {
            "%INPUT%" => format!("{}/{}", temp_dir, language.file_name),
            "%OUTPUT%" => format!("{}/{}", temp_dir, "target"),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()[1..];

    //Compiles the source code in a child process and records the time it took
    let compile_instant = Instant::now();
    let mut compile_child = Command::new(&language.command[0])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut compile_time;
    'compile_time_measure: loop {
        compile_time = compile_instant.elapsed();
        match compile_child.try_wait()? {
            Some(_) => {
                break 'compile_time_measure;
            }
            None => {}
        }
    }

    //Collects the result
    let output = compile_child.wait_with_output()?;

    //Checks whether the compilation has succeeded
    if !output.status.success() {
        result = OjResult::CompilationError;
        case_results.push(CaseResult {
            id: 0,
            result: OjResult::CompilationError,
            time: compile_time.as_micros(),
            memory: 0,
            info: String::from_utf8(output.stderr)?,
        });
        for j in 1..=problem.cases.len() {
            case_results.push(CaseResult {
                id: j,
                result: OjResult::Waiting,
                time: 0,
                memory: 0,
                info: "".to_string(),
            });
        }
    } else {
        //Records the result of compilation
        case_results.push(CaseResult {
            id: 0,
            result: OjResult::CompilationSuccess,
            time: compile_time.as_micros(),
            memory: 0,
            info: "".to_string(),
        });

        //Runs each case
        'cases: for (i, case) in problem.cases.iter().enumerate() {
            //Prepares the input, output and the answer
            let infile = fs::File::open(&case.input_file)?;
            let outfile = fs::File::create(format!("{}/{}", temp_dir, "output"))?;
            let answer = fs::read_to_string(&case.answer_file)?;

            //Runs the case in a child process and records the time it took
            let run_instant = Instant::now();
            let mut run_time;
            let mut run_child = Command::new(format!("{}/{}", temp_dir, "target"))
                .stdin(Stdio::from(infile))
                .stdout(Stdio::from(outfile))
                .stderr(Stdio::piped())
                .spawn()?;
            'run_time_measure: loop {
                run_time = run_instant.elapsed();
                if case.time_limit != 0 && run_time > Duration::from_micros(case.time_limit) {
                    run_child.kill()?;
                    result = match result {
                        OjResult::Accepted => OjResult::TimeLimitExceeded,
                        result => result,
                    };
                    case_results.push(CaseResult {
                        id: i + 1,
                        result: OjResult::TimeLimitExceeded,
                        time: case.time_limit as u128,
                        memory: 0,
                        info: format!("Time limit: {}", case.time_limit),
                    });
                    continue 'cases;
                }
                match run_child.try_wait()? {
                    Some(_) => {
                        break 'run_time_measure;
                    }
                    None => {}
                }
            }

            //Collects the result
            let output = run_child.wait_with_output()?;
            let stdout = fs::read_to_string(format!("{}/{}", temp_dir, "output"))?;
            let stderr = String::from_utf8(output.stderr)?;

            //Checks whether runtime error occurred
            if !output.status.success() {
                result = match result {
                    OjResult::Accepted => OjResult::RuntimeError,
                    result => result,
                };
                case_results.push(CaseResult {
                    id: i + 1,
                    result: OjResult::RuntimeError,
                    time: run_time.as_micros(),
                    memory: 0,
                    info: stderr,
                });
            } else {
                //Judges the result according to the problem type
                match problem.problem_type {
                    ProblemType::Standard | ProblemType::DynamicRanking => {
                        if stdout
                            .split('\n')
                            .map(|l| l.trim())
                            .zip(answer.split('\n').map(|l| l.trim()))
                            .fold(true, |acc, (l, r)| if acc && l == r { true } else { false })
                        {
                            score += case.score;
                            case_results.push(CaseResult {
                                id: i + 1,
                                result: OjResult::Accepted,
                                time: run_time.as_micros(),
                                memory: 0,
                                info: stdout,
                            });
                        } else {
                            result = match result {
                                OjResult::Accepted => OjResult::WrongAnswer,
                                result => result,
                            };
                            case_results.push(CaseResult {
                                id: i + 1,
                                result: OjResult::WrongAnswer,
                                time: run_time.as_micros(),
                                memory: 0,
                                info: stdout,
                            });
                        }
                    }
                    ProblemType::Strict => {
                        if stdout == answer {
                            score += case.score;
                            case_results.push(CaseResult {
                                id: i + 1,
                                result: OjResult::Accepted,
                                time: run_time.as_micros(),
                                memory: 0,
                                info: stdout,
                            });
                        } else {
                            result = match result {
                                OjResult::Accepted => OjResult::WrongAnswer,
                                result => result,
                            };
                            case_results.push(CaseResult {
                                id: i + 1,
                                result: OjResult::WrongAnswer,
                                time: run_time.as_micros(),
                                memory: 0,
                                info: stdout,
                            });
                        }
                    }
                    ProblemType::Spj => match &problem.misc.special_judge {
                        Some(cmd) => {
                            let args = &cmd
                                .iter()
                                .map(|arg| match arg.as_str() {
                                    "%ANSWER%" => case.answer_file.clone(),
                                    "%OUTPUT%" => format!("{}/{}", temp_dir, "output"),
                                    other => other.to_string(),
                                })
                                .collect::<Vec<_>>()[1..];
                            let output = Command::new(&cmd[0]).args(args).output()?;
                            if !output.status.success() {
                                result = match result {
                                    OjResult::Accepted => OjResult::SpjError,
                                    result => result,
                                };
                                case_results.push(CaseResult {
                                    id: i + 1,
                                    result: OjResult::SpjError,
                                    time: run_time.as_micros(),
                                    memory: 0,
                                    info: "Error occurred while calling the special judger"
                                        .to_string(),
                                })
                            } else {
                                let stdout = String::from_utf8(output.stdout)?
                                    .split('\n')
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty())
                                    .collect::<Vec<_>>();
                                if stdout.len() != 2 {
                                    result = match result {
                                        OjResult::Accepted => OjResult::SpjError,
                                        result => result,
                                    };
                                    case_results.push(CaseResult {
                                        id: i + 1,
                                        result: OjResult::SpjError,
                                        time: run_time.as_micros(),
                                        memory: 0,
                                        info: "Invalid special judge output.".to_string(),
                                    })
                                } else {
                                    match serde_json::from_str(&format!("\"{}\"", &stdout[0])) {
                                        Ok(spj_result) => match spj_result {
                                            OjResult::Accepted => {
                                                score += case.score;
                                                case_results.push(CaseResult {
                                                    id: i + 1,
                                                    result: OjResult::Accepted,
                                                    time: run_time.as_micros(),
                                                    memory: 0,
                                                    info: stdout[1].clone(),
                                                })
                                            }
                                            other => {
                                                result = match spj_result {
                                                    OjResult::Accepted => other,
                                                    result => result,
                                                };
                                                case_results.push(CaseResult {
                                                    id: i + 1,
                                                    result: other,
                                                    time: run_time.as_micros(),
                                                    memory: 0,
                                                    info: stdout[1].clone(),
                                                })
                                            }
                                        },
                                        Err(_) => case_results.push(CaseResult {
                                            id: i + 1,
                                            result: OjResult::SpjError,
                                            time: run_time.as_micros(),
                                            memory: 0,
                                            info: "Invalid special judge output.".to_string(),
                                        }),
                                    }
                                }
                            }
                        }
                        None => case_results.push(CaseResult {
                            id: i + 1,
                            result: OjResult::SpjError,
                            time: run_time.as_micros(),
                            memory: 0,
                            info: "Special judge command not found".to_string(),
                        }),
                    },
                }
            }
        }
    }

    //Cleans up
    fs::remove_dir_all(&temp_dir)?;

    Ok(Job {
        id,
        created_time,
        updated_time,
        submission: submission.clone(),
        state: OjState::Finished,
        result,
        score,
        cases: case_results,
    })
}

///POST requests for "/users" handler
#[post("/users")]
async fn post_users(
    user: web::Json<User>,
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let user = user.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "post_users_handler", "Handling POST for user {}", user.name);

    //Checks if the user name already exists
    match oj_try!(User::select_by_name(&user.name, &pool)) {
        Some(_) => HttpResponse::BadRequest().body(
            serde_json::to_string(&ErrorResponseBody {
                code: 1,
                reason: ErrorReason::ErrInvalidArgument,
                message: format!("User name '{}' already exists.", user.name),
            })
            .unwrap(),
        ),
        None => match user.id {
            //If id is provided then does update
            Some(id) => match oj_try!(User::select_by_id(id, &pool)) {
                Some(_) => {
                    oj_try!(user.update(&pool));
                    HttpResponse::Ok().body(serde_json::to_string(&user).unwrap())
                }
                None => HttpResponse::NotFound().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 3,
                        reason: ErrorReason::ErrNotFound,
                        message: format!("User {} not found.", id),
                    })
                    .unwrap(),
                ),
            },
            //Otherwise does insert
            None => {
                oj_try!(user.insert(&pool));
                HttpResponse::Ok().body(
                    serde_json::to_string(&oj_try!(User::select_by_name(&user.name, &pool)))
                        .unwrap(),
                )
            }
        },
    }
}

///POST requests for "/contests" handler
#[post("/contests")]
async fn post_contests(
    contest: web::Json<Contest>,
    config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let contest = contest.into_inner();
    let config = config.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "post_contests_handler", "Handling POST for contest {}", contest.name);

    //Checks whether the specified problems and users exist
    if !contest.problem_ids.iter().fold(true, |acc, pid| {
        if config
            .problems
            .iter()
            .map(|problem| problem.id)
            .collect::<Vec<_>>()
            .contains(pid)
            && acc
        {
            true
        } else {
            false
        }
    }) || !{
        oj_try!(contest.user_ids.iter().fold(Ok(true), |acc, uid| {
            if match User::select_all(&pool) {
                Ok(v) => v,
                Err(e) => {
                    return Err(e);
                }
            }
            .iter()
            .map(|user| user.id.unwrap())
            .collect::<Vec<_>>()
            .contains(uid)
                && acc.unwrap()
            {
                Ok(true)
            } else {
                Ok(false)
            }
        }))
    } {
        return HttpResponse::NotFound().body(
            serde_json::to_string(&ErrorResponseBody {
                code: 3,
                reason: ErrorReason::ErrNotFound,
                message: format!(
                    "Contest {} not found.",
                    match contest.id {
                        Some(id) => id,
                        None => oj_try!(Contest::count(&pool)) + 1,
                    }
                ),
            })
            .unwrap(),
        );
    }

    match contest.id {
        //If id is provided then does update
        Some(id) => match oj_try!(Contest::select_by_id(id, &pool)) {
            Some(_) => {
                oj_try!(contest.update(&pool));
                HttpResponse::Ok().body(serde_json::to_string(&contest).unwrap())
            }
            None => HttpResponse::NotFound().body(
                serde_json::to_string(&ErrorResponseBody {
                    code: 3,
                    reason: ErrorReason::ErrNotFound,
                    message: format!("Contest {} not found.", id),
                })
                .unwrap(),
            ),
        },
        //Otherwise does insert
        None => {
            oj_try!(contest.insert(&pool));
            HttpResponse::Ok().body(
                serde_json::to_string(&oj_try!(Contest::select_by_name(&contest.name, &pool)))
                    .unwrap(),
            )
        }
    }
}

///GET requests for "/jobs" handler
#[get("/jobs")]
async fn get_jobs(
    query: web::Query<Filter>,
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    log::info!(target: "get_jobs_handler", "Handling GET for jobs");

    //Unwraps the arguments
    let query = query.into_inner();
    let pool = pool.into_inner();

    //Filters the jobs
    HttpResponse::Ok().body(serde_json::to_string(&oj_try!(query.apply(&pool))).unwrap())
}

///GET requests for "/users" handler
#[get("/users")]
async fn get_users(
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let pool = pool.into_inner();

    log::info!(target: "get_users_handler", "Handling GET for users");

    HttpResponse::Ok().body(serde_json::to_string(&oj_try!(User::select_all(&pool))).unwrap())
}

///GET requests for "/contests" handler
#[get("/contests")]
async fn get_contests(
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let pool = pool.into_inner();

    log::info!(target: "get_contests_handler", "Handling GET for contests");

    HttpResponse::Ok().body(serde_json::to_string(&oj_try!(Contest::select_all(&pool))).unwrap())
}

///GET requests for "/jobs/{jobId}" handler
#[get("/jobs/{jobId}")]
async fn get_jobs_by_id(
    path: web::Path<usize>,
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let id = path.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "get_jobs_by_id_handler", "Handling GET for job {}", id);

    //Selects the chosen job
    let job = oj_try!(Job::select_by_id(id, &pool));
    match job {
        Some(job) => HttpResponse::Ok().body(serde_json::to_string(&job).unwrap()),
        None => {
            return HttpResponse::NotFound().body(
                serde_json::to_string(&ErrorResponseBody {
                    code: 3,
                    reason: ErrorReason::ErrNotFound,
                    message: format!("Job {} not found.", id),
                })
                .unwrap(),
            );
        }
    }
}

///GET requests for "/contests/{contestId}" handler
#[get("/contests/{contestId}")]
async fn get_contests_by_id(
    path: web::Path<usize>,
    _config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let id = path.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "get_contests_by_id_handler", "Handling GET for contest {}", id);

    //Selects the chosen contest
    let contest = oj_try!(Contest::select_by_id(id, &pool));
    match contest {
        Some(contest) => HttpResponse::Ok().body(serde_json::to_string(&contest).unwrap()),
        None => {
            return HttpResponse::NotFound().body(
                serde_json::to_string(&ErrorResponseBody {
                    code: 3,
                    reason: ErrorReason::ErrNotFound,
                    message: format!("Contest {} not found.", id),
                })
                .unwrap(),
            );
        }
    }
}

///GET requests for "/contests/{contestId}/ranklist" handler
#[get("/contests/{contestId}/ranklist")]
async fn get_contests_ranklist(
    path: web::Path<usize>,
    rule: web::Query<RankingRule>,
    config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let id = path.into_inner();
    let rule = rule.into_inner();
    let config = config.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "get_contests_ranklist_handler", "Handling GET for contest {}", id);

    //Declares the variables to be used
    let user_ids;
    let problem_ids;
    let mut usersranking = vec![];

    match oj_try!(Contest::select_by_id(id, &pool)) {
        //If id provided is not 0 and the contest with the id exists then ranks the specified contest
        Some(contest) => {
            user_ids = contest.user_ids.clone();
            problem_ids = contest.problem_ids.clone();
        }
        None => {
            //If id provided is 0 then ranks globally
            if id == 0 {
                user_ids = (0..oj_try!(User::count(&pool))).collect();
                problem_ids = config.problems.iter().map(|p| p.id).collect();
            } else {
                //Otherwise raises error
                return HttpResponse::NotFound().body(
                    serde_json::to_string(&ErrorResponseBody {
                        code: 3,
                        reason: ErrorReason::ErrNotFound,
                        message: format!("Contest {} not found.", id),
                    })
                    .unwrap(),
                );
            }
        }
    };

    //Processes the data of each user
    for user_id in user_ids {
        //Declares the variables to be used
        let mut scores = vec![];
        let mut max_time = UtcDateTime {
            time: DateTime::<Utc>::MIN_UTC,
        };
        let mut submission_count = 0;

        for problem_id in &problem_ids {
            //Gets all jobs conform to the constraints
            let filtered_jobs = oj_try!(Filter {
                user_id: Some(user_id),
                contest_id: Some(id),
                problem_id: Some(*problem_id),
                ..Default::default()
            }
            .apply(&pool));

            //Gets the current problem
            let problem = config
                .problems
                .iter()
                .find(|problem| problem.id == *problem_id)
                .unwrap();

            //Different ranking methods
            if problem.problem_type == ProblemType::DynamicRanking {
                let dynamic_ranking_ratio = match problem.misc.dynamic_ranking_ratio {
                    Some(ratio) => ratio,
                    None => {
                        return HttpResponse::BadRequest().body(
                            serde_json::to_string(&ErrorResponseBody {
                                code: 1,
                                reason: ErrorReason::ErrInvalidArgument,
                                message: format!(
                                    "Dynamic ranking ratio of problem {} not found.",
                                    problem.id
                                ),
                            })
                            .unwrap(),
                        );
                    }
                };

                let accepted_jobs = filtered_jobs
                    .iter()
                    .filter(|job| job.result == OjResult::Accepted)
                    .collect::<Vec<_>>();

                //If there is not any accepted submissions then selects the valid job according to the scoring rule
                //Otherwise selects the latest accepted submission
                if accepted_jobs.len() == 0 {
                    let job = match rule.scoring_rule.unwrap_or(ScoringRule::Latest) {
                        ScoringRule::Latest => {
                            filtered_jobs.iter().max_by_key(|job| *job.created_time)
                        }
                        ScoringRule::Highest => filtered_jobs
                            .iter()
                            .max_by(|l, r| l.score.partial_cmp(&r.score).unwrap()),
                    };

                    match job {
                        Some(job) => {
                            scores.push(job.score * (1.0 - dynamic_ranking_ratio));
                            max_time = if *job.created_time > *max_time {
                                job.created_time
                            } else {
                                max_time
                            };
                        }
                        None => {
                            scores.push(0.0);
                        }
                    }
                } else {
                    let mut score = 0.0;
                    let all_accepted_jobs = oj_try!(Filter {
                        contest_id: Some(id),
                        problem_id: Some(*problem_id),
                        ..Default::default()
                    }
                    .apply(&pool));
                    let job = filtered_jobs
                        .iter()
                        .max_by_key(|job| *job.created_time)
                        .unwrap();

                    //Calculates the score of each case dynamically
                    for i in 0..problem.cases.len() {
                        score += problem.cases[i].score / job.cases[i + 1].time as f32
                            * all_accepted_jobs
                                .iter()
                                .map(|job| job.cases[i + 1].time)
                                .min()
                                .unwrap() as f32
                            * dynamic_ranking_ratio
                            + problem.cases[i].score * (1.0 - dynamic_ranking_ratio);
                    }

                    scores.push(score);
                    max_time = if *job.created_time > *max_time {
                        job.created_time
                    } else {
                        max_time
                    };
                }
            } else {
                //Selects the valid job according to the scoring rule
                let job = match rule.scoring_rule.unwrap_or(ScoringRule::Latest) {
                    ScoringRule::Latest => filtered_jobs.iter().max_by_key(|job| *job.created_time),
                    ScoringRule::Highest => filtered_jobs
                        .iter()
                        .max_by(|l, r| l.score.partial_cmp(&r.score).unwrap()),
                };

                match job {
                    Some(job) => {
                        scores.push(job.score);
                        max_time = if *job.created_time > *max_time {
                            job.created_time
                        } else {
                            max_time
                        };
                    }
                    None => {
                        scores.push(0.0);
                    }
                }
            }

            submission_count += filtered_jobs.len();
        }

        usersranking.push(UsersRanking {
            user: oj_try!(User::select_by_id(user_id, &pool)).unwrap(),
            rank: 0,
            scores,
            max_time: if submission_count == 0 {
                UtcDateTime {
                    time: DateTime::<Utc>::MAX_UTC,
                }
            } else {
                max_time
            },
            submission_count,
        });
    }

    //Breaks the ties according to the given rule
    usersranking.sort_by(|l, r| {
        match l
            .scores
            .iter()
            .fold(0.0, |acc, s| acc + s)
            .partial_cmp(&r.scores.iter().fold(0.0, |acc, s| acc + s))
            .unwrap()
        {
            Ordering::Equal => match rule.tie_breaker.unwrap_or(TieBreaker::UserId) {
                TieBreaker::SubmissionTime => match l.max_time.cmp(&r.max_time).reverse() {
                    Ordering::Equal => l.user.id.cmp(&r.user.id).reverse(),
                    other => other,
                },
                TieBreaker::SubmissionCount => {
                    match l.submission_count.cmp(&r.submission_count).reverse() {
                        Ordering::Equal => l.user.id.cmp(&r.user.id).reverse(),
                        other => other,
                    }
                }
                TieBreaker::UserId => l.user.id.cmp(&r.user.id).reverse(),
            },
            other => other,
        }
    });

    usersranking.reverse();

    //Assigns the rank of each user
    if usersranking.len() == 1 {
        usersranking[0].rank = 1;
    }
    for i in 0..usersranking.len() - 1 {
        if usersranking[i].scores.iter().fold(0.0, |acc, s| acc + s)
            == usersranking[i + 1]
                .scores
                .iter()
                .fold(0.0, |acc, s| acc + s)
        {
            match rule.tie_breaker {
                Some(tie_breaker) => match tie_breaker {
                    TieBreaker::SubmissionTime => {
                        if *usersranking[i].max_time == *usersranking[i + 1].max_time {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = usersranking[i].rank;
                        } else {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = i + 2;
                        }
                    }
                    TieBreaker::SubmissionCount => {
                        if usersranking[i].submission_count == usersranking[i + 1].submission_count
                        {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = usersranking[i].rank;
                        } else {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = i + 2;
                        }
                    }
                    TieBreaker::UserId => {
                        if usersranking[i].user.id == usersranking[i + 1].user.id {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = usersranking[i].rank;
                        } else {
                            usersranking[i].rank = if usersranking[i].rank == 0 {
                                i + 1
                            } else {
                                usersranking[i].rank
                            };
                            usersranking[i + 1].rank = i + 2;
                        }
                    }
                },
                None => {
                    usersranking[i].rank = if usersranking[i].rank == 0 {
                        i + 1
                    } else {
                        usersranking[i].rank
                    };
                    usersranking[i + 1].rank = usersranking[i].rank;
                }
            }
        } else {
            usersranking[i].rank = if usersranking[i].rank == 0 {
                i + 1
            } else {
                usersranking[i].rank
            };
            usersranking[i + 1].rank = i + 2;
        }
    }

    HttpResponse::Ok().body(serde_json::to_string(&usersranking).unwrap())
}

///PUT requests for "/jobs/{jobId}" handler
#[put("/jobs/{jobId}")]
async fn put_jobs_by_id(
    path: web::Path<usize>,
    config: web::Data<Config>,
    pool: web::Data<Pool<SqliteConnectionManager>>,
) -> impl Responder {
    //Unwraps the arguments
    let id = path.into_inner();
    let config = config.into_inner();
    let pool = pool.into_inner();

    log::info!(target: "put_jobs_by_id_handler", "Handling PUT for job {}", id);

    //Gets the original job
    let original_job = match oj_try!(Job::select_by_id(id, &pool)) {
        Some(job) => job,
        None => {
            return HttpResponse::NotFound().body(
                serde_json::to_string(&ErrorResponseBody {
                    code: 3,
                    reason: ErrorReason::ErrNotFound,
                    message: format!("Job {} not found.", id),
                })
                .unwrap(),
            );
        }
    };

    //Does rejudging
    let updated_time = UtcDateTime { time: Utc::now() };
    let job = oj_try!(judge(
        id,
        &original_job.submission,
        config.clone(),
        original_job.created_time,
        updated_time,
    ));

    //Stores to the SQLite database
    oj_try!(job.update(&pool));

    HttpResponse::Ok().body(serde_json::to_string(&job).unwrap())
}

//Used in automatic testing
#[post("/internal/exit")]
#[allow(unreachable_code)]
async fn exit() -> impl Responder {
    log::info!("Shutdown as requested");
    std::process::exit(0);
    format!("Exited")
}

#[actix_web::main]
async fn main() -> io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    //Initializes the configuration
    let args: Cli = Cli::parse();
    let config: Config = serde_json::from_str(&fs::read_to_string(match args.config {
        Some(ref filename) => filename,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path to the configuration file missing.",
            ))
        }
    })?)?;

    //Checks the configuration
    for p1 in &config.problems {
        if config.problems.iter().filter(|p2| p1.id == p2.id).count() > 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Conflicting problem ID",
            ));
        }
    }

    for l1 in &config.languages {
        if config
            .languages
            .iter()
            .filter(|l2| l1.name == l2.name)
            .count()
            > 1
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Conflicting problem ID",
            ));
        }
    }

    //Flushes the data if required
    if args.flush_data {
        let _ = fs::remove_file("oj.db");
    }

    //Initializes database
    let manager = SqliteConnectionManager::file("oj.db");
    let pool = Pool::new(manager).unwrap();
    database_init(&pool).unwrap();

    //Cleans up
    let _ = fs::remove_dir_all("temp");

    //Starts the server
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .app_data(web::Data::new(config.clone()))
            .app_data(web::Data::new(pool.clone()))
            .service(post_jobs)
            .service(get_jobs)
            .service(get_jobs_by_id)
            .service(put_jobs_by_id)
            .service(post_users)
            .service(get_users)
            .service(post_contests)
            .service(get_contests_by_id)
            .service(get_contests)
            .service(get_contests_ranklist)
            //Used in automatic testing
            .service(exit)
    })
    .bind(("127.0.0.1", 12345))?
    .run()
    .await?;
    Ok(())
}
