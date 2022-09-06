use super::{date_time_format::*, *};
use chrono::prelude::*;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use Error;

///SQLite database initialization
pub fn database_init(pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
    pool.get()?.execute(
        "CREATE TABLE IF NOT EXISTS jobs (
            id                  INTEGER PRIMARY KEY,
            created_time        TEXT NOT NULL,
            updated_time        TEXT NOT NULL,
            source_code         TEXT NOT NULL,
            language            TEXT NOT NULL,
            user_id             INTEGER,
            problem_id          INTEGER,
            contest_id          INTEGER,
            state               TEXT NOT NULL,
            result              TEXT NOT NULL,
            score               INTEGER,
            cases               TEXT NOT NULL
        )",
        [],
    )?;
    pool.get()?.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id                  INTEGER PRIMARY KEY,
            name                TEXT NOT NULL
        )",
        [],
    )?;
    pool.get()?.execute(
        "CREATE TABLE IF NOT EXISTS contests (
            id                      INTEGER PRIMARY KEY,
            name                    TEXT NOT NULL,
            from_time               TEXT NOT NULL,
            to_time                 TEXT NOT NULL,
            problem_ids             TEXT NOT NULL,
            user_ids                TEXT NOT NULL,
            submission_limit        INTEGER
        )",
        [],
    )?;
    match User::select_by_name("root", pool)? {
        Some(_) => {}
        None => User {
            id: Some(0),
            name: "root".to_string(),
        }
        .insert(pool)?,
    };

    Ok(())
}

impl Job {
    ///Inserts a job into the SQLite database
    pub fn insert(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "INSERT INTO jobs (
                id,
                created_time,
                updated_time,
                source_code,
                language,
                user_id,
                problem_id,
                contest_id,
                state,
                result,
                score,
                cases
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
            )",
            params![
                self.id,
                self.created_time.format(FORMAT).to_string(),
                self.updated_time.format(FORMAT).to_string(),
                self.submission.source_code,
                self.submission.language,
                self.submission.user_id,
                self.submission.problem_id,
                self.submission.contest_id,
                serde_json::to_string(&self.state)?,
                serde_json::to_string(&self.result)?,
                self.score,
                serde_json::to_string(&self.cases)?
            ],
        )?;

        Ok(())
    }

    ///Selects all the jobs in the SQLite database
    pub fn select_all(pool: &Pool<SqliteConnectionManager>) -> Result<Vec<Self>, Box<dyn Error>> {
        let conn = pool.get()?;
        let mut stmt = conn.prepare("SELECT * from jobs")?;
        let iter = stmt.query_map(params![], |row| {
            Ok(Self {
                id: row.get(0)?,
                created_time: UtcDateTime {
                    time: match Utc.datetime_from_str(
                        &match row.get::<_, String>(1) {
                            Ok(s) => s,
                            Err(_) => return Err(rusqlite::Error::InvalidQuery),
                        },
                        FORMAT,
                    ) {
                        Ok(t) => t,
                        Err(_) => return Err(rusqlite::Error::InvalidQuery),
                    },
                },
                updated_time: UtcDateTime {
                    time: match Utc.datetime_from_str(
                        &match row.get::<_, String>(2) {
                            Ok(s) => s,
                            Err(_) => return Err(rusqlite::Error::InvalidQuery),
                        },
                        FORMAT,
                    ) {
                        Ok(t) => t,
                        Err(_) => return Err(rusqlite::Error::InvalidQuery),
                    },
                },
                submission: Submission {
                    source_code: row.get(3)?,
                    language: row.get(4)?,
                    user_id: row.get(5)?,
                    problem_id: row.get(6)?,
                    contest_id: row.get(7)?,
                },
                state: match serde_json::from_str(&row.get::<_, String>(8)?) {
                    Ok(s) => s,
                    Err(_) => return Err(rusqlite::Error::InvalidQuery),
                },
                result: match serde_json::from_str(&row.get::<_, String>(9)?) {
                    Ok(s) => s,
                    Err(_) => return Err(rusqlite::Error::InvalidQuery),
                },
                score: row.get(10)?,
                cases: match serde_json::from_str(&row.get::<_, String>(11)?) {
                    Ok(s) => s,
                    Err(_) => return Err(rusqlite::Error::InvalidQuery),
                },
            })
        })?;
        Ok(iter.collect::<rusqlite::Result<Vec<Self>>>()?)
    }

    ///Gets a job by its id
    pub fn select_by_id(
        id: usize,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(Self::select_all(pool)?.into_iter().find(|job| job.id == id))
    }

    ///Gets the count of all the jobs in the SQLite database
    pub fn count(pool: &Pool<SqliteConnectionManager>) -> Result<usize, Box<dyn Error>> {
        Ok(pool
            .get()?
            .query_row("SELECT COUNT(*) FROM jobs", params![], |row| row.get(0))?)
    }

    ///Updates the specified job
    pub fn update(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "UPDATE jobs SET
            created_time = ?1,
            updated_time = ?2,
            source_code = ?3,
            language = ?4,
            user_id = ?5,
            problem_id = ?6,
            contest_id = ?7,
            state = ?8,
            result = ?9,
            score = ?10,
            cases = ?11
            WHERE id = ?12",
            params![
                self.created_time.format(FORMAT).to_string(),
                self.updated_time.format(FORMAT).to_string(),
                self.submission.source_code,
                self.submission.language,
                self.submission.user_id,
                self.submission.problem_id,
                self.submission.contest_id,
                serde_json::to_string(&self.state)?,
                serde_json::to_string(&self.result)?,
                self.score,
                serde_json::to_string(&self.cases)?,
                self.id,
            ],
        )?;

        Ok(())
    }
}

impl User {
    ///Inserts a user into the SQLite database
    pub fn insert(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "INSERT INTO users (
            id,
            name
        ) VALUES (
            ?1,
            ?2
        )",
            params![
                match self.id {
                    Some(id) => id,
                    None => Self::count(&pool)?,
                },
                self.name
            ],
        )?;
        Ok(())
    }

    ///Selects all the users in the SQLite database
    pub fn select_all(pool: &Pool<SqliteConnectionManager>) -> Result<Vec<Self>, Box<dyn Error>> {
        let conn = pool.get()?;
        let mut stmt = conn.prepare("SELECT * from users")?;
        let iter = stmt.query_map(params![], |row| {
            Ok(Self {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        Ok(iter.collect::<rusqlite::Result<Vec<Self>>>()?)
    }

    ///Gets a user by its id
    pub fn select_by_id(
        id: usize,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(Self::select_all(pool)?
            .into_iter()
            .find(|user| user.id.unwrap() == id))
    }

    //Gets a user by its name
    pub fn select_by_name(
        name: &str,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(Self::select_all(pool)?
            .into_iter()
            .find(|user| user.name == name))
    }

    ///Gets the count of all the users in the SQLite database
    pub fn count(pool: &Pool<SqliteConnectionManager>) -> Result<usize, Box<dyn Error>> {
        Ok(pool
            .get()?
            .query_row("SELECT COUNT(*) FROM users", params![], |row| row.get(0))?)
    }

    ///Updates the specified user
    pub fn update(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "UPDATE users SET
        name = ?1
        WHERE id = ?2",
            params![self.name, self.id.unwrap()],
        )?;
        Ok(())
    }
}

impl Contest {
    ///Inserts a contest into the SQLite database
    pub fn insert(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "INSERT INTO contests (
                name,
                from_time,
                to_time,
                problem_ids,
                user_ids,
                submission_limit
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6
            )",
            params![
                self.name,
                self.from.format(FORMAT).to_string(),
                self.to.format(FORMAT).to_string(),
                serde_json::to_string(&self.problem_ids)?,
                serde_json::to_string(&self.user_ids)?,
                self.submission_limit
            ],
        )?;
        Ok(())
    }

    ///Selects all the contests in the SQLite database
    pub fn select_all(pool: &Pool<SqliteConnectionManager>) -> Result<Vec<Self>, Box<dyn Error>> {
        let conn = pool.get()?;
        let mut stmt = conn.prepare("SELECT * from contests")?;
        let iter = stmt.query_map(params![], |row| {
            Ok(Self {
                id: row.get(0)?,
                name: row.get(1)?,
                from: UtcDateTime {
                    time: match Utc.datetime_from_str(
                        &match row.get::<_, String>(2) {
                            Ok(s) => s,
                            Err(_) => return Err(rusqlite::Error::InvalidQuery),
                        },
                        FORMAT,
                    ) {
                        Ok(t) => t,
                        Err(_) => return Err(rusqlite::Error::InvalidQuery),
                    },
                },
                to: UtcDateTime {
                    time: match Utc.datetime_from_str(
                        &match row.get::<_, String>(3) {
                            Ok(s) => s,
                            Err(_) => return Err(rusqlite::Error::InvalidQuery),
                        },
                        FORMAT,
                    ) {
                        Ok(t) => t,
                        Err(_) => return Err(rusqlite::Error::InvalidQuery),
                    },
                },
                problem_ids: match serde_json::from_str(&row.get::<_, String>(4)?) {
                    Ok(s) => s,
                    Err(_) => return Err(rusqlite::Error::InvalidQuery),
                },
                user_ids: match serde_json::from_str(&row.get::<_, String>(5)?) {
                    Ok(s) => s,
                    Err(_) => return Err(rusqlite::Error::InvalidQuery),
                },
                submission_limit: row.get(6)?,
            })
        })?;
        Ok(iter.collect::<rusqlite::Result<Vec<Contest>>>()?)
    }

    ///Gets the count of all the contests in the SQLite database
    pub fn count(pool: &Pool<SqliteConnectionManager>) -> Result<usize, Box<dyn Error>> {
        Ok(pool
            .get()?
            .query_row("SELECT COUNT(*) FROM contests", params![], |row| row.get(0))?)
    }

    ///Updates the specified contest
    pub fn update(&self, pool: &Pool<SqliteConnectionManager>) -> Result<(), Box<dyn Error>> {
        pool.get()?.execute(
            "UPDATE contests SET
            name = ?1,
            from_time = ?2,
            to_time = ?3,
            problem_ids = ?4,
            user_ids = ?5,
            submission_limit = ?6
            WHERE id = ?7",
            params![
                self.name,
                self.from.format(FORMAT).to_string(),
                self.to.format(FORMAT).to_string(),
                serde_json::to_string(&self.problem_ids)?,
                serde_json::to_string(&self.user_ids)?,
                self.submission_limit,
                self.id,
            ],
        )?;
        Ok(())
    }

    ///Gets a contest by its id
    pub fn select_by_id(
        id: usize,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(Self::select_all(pool)?
            .into_iter()
            .find(|contest| contest.id.unwrap() == id))
    }

    //Gets a contest by its name
    pub fn select_by_name(
        name: &str,
        pool: &Pool<SqliteConnectionManager>,
    ) -> Result<Option<Self>, Box<dyn Error>> {
        Ok(Self::select_all(pool)?
            .into_iter()
            .find(|contest| contest.name == name))
    }
}
