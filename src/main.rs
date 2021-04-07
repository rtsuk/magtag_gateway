use anyhow::{Context, Error, Result};
use chrono::{DateTime, Local, Utc};
use chrono_tz::US::Pacific;
use log::info;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self},
    path::PathBuf,
};
use structopt::StructOpt;

const ONE_MINUTE_IN_SECONDS: i64 = 60;
const FIFTEEN_MINUTES_IN_SECONDS: i64 = 15 * ONE_MINUTE_IN_SECONDS;
const TWENTY_MINUTES_IN_SECONDS: i64 = 20 * ONE_MINUTE_IN_SECONDS;

#[derive(StructOpt, Debug)]
#[structopt(name = "magtag_gateway")]
struct Opt {
    #[structopt(short, long)]
    verbose: bool,

    #[structopt(short, long)]
    line: Option<PathBuf>,

    #[structopt(short, long)]
    next: Option<PathBuf>,

    #[structopt(short, long)]
    team: Option<usize>,
}

const SHARKS_ID: usize = 28;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub id: usize,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TeamAtGame {
    pub score: Option<usize>,
    pub team: Team,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Teams {
    pub home: TeamAtGame,
    pub away: TeamAtGame,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    abstract_game_state: String,
    detailed_state: String,
}

impl Status {
    fn is_preview(&self) -> bool {
        self.abstract_game_state == "Preview"
    }

    fn is_pregame(&self) -> bool {
        self.detailed_state == "Pre-Game"
    }

    fn is_live(&self) -> bool {
        self.abstract_game_state == "Live"
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IntermissionInfo {
    in_intermission: bool,
    intermission_time_remaining: usize,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Linescore {
    current_period: usize,
    current_period_ordinal: Option<String>,
    current_period_time_remaining: Option<String>,
    intermission_info: Option<IntermissionInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub game_date: DateTime<Utc>,
    pub teams: Teams,
    pub status: Status,
    pub linescore: Option<Linescore>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameDate {
    date: String,
    games: Vec<Game>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NextGameSchedule {
    total_items: usize,
    dates: Vec<GameDate>,
}

impl NextGameSchedule {
    fn game_today(&self, utc_now: &DateTime<Utc>) -> bool {
        if self.total_items < 1 {
            return false;
        }
        let game_date = &self.dates[0];
        let game = &game_date.games[0];
        let game_date_pacific = game.game_date.with_timezone(&Pacific);
        let pacific_now = utc_now.with_timezone(&Pacific);
        game_date_pacific.date() == pacific_now.date()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTeam {
    pub id: usize,
    pub name: String,
    pub next_game_schedule: NextGameSchedule,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub teams: Vec<ScheduledTeam>,
}

#[derive(Serialize, Deserialize, Debug)]
struct NextUp {
    top: String,
    middle: String,
    bottom: String,
    time: String,
    sleep: i64,
}

fn opponent_name(teams: &Teams, home_team: usize) -> String {
    if teams.home.team.id == home_team {
        format!("vs {}", teams.away.team.name)
    } else {
        format!("@ {}", teams.home.team.name)
    }
}

fn format_date_time(date_time: &DateTime<chrono_tz::Tz>) -> String {
    date_time.format("%-I:%M%p").to_string()
}

fn format_game_time_relative(
    date_time: &DateTime<chrono_tz::Tz>,
    utc_now: &DateTime<chrono_tz::Tz>,
) -> String {
    let date_pacific = date_time.with_timezone(&Pacific);
    let pacific_now = utc_now.with_timezone(&Pacific);
    if date_pacific.date() == pacific_now.date() {
        date_time.format("Today @ %-I:%M%p").to_string()
    } else {
        date_time.format("%b %-d @ %-I:%M%p").to_string()
    }
}

fn sleep_time(date_time: &DateTime<chrono_tz::Tz>, utc_now: &DateTime<chrono_tz::Tz>) -> i64 {
    let duration_until_game = *date_time - *utc_now;

    if duration_until_game.num_seconds() < 0
        || duration_until_game.num_seconds() > TWENTY_MINUTES_IN_SECONDS
    {
        FIFTEEN_MINUTES_IN_SECONDS
    } else {
        ONE_MINUTE_IN_SECONDS
    }
}

impl NextUp {
    fn new(
        linescore_response_string: &str,
        next_response_string: &str,
        team_id: usize,
        utc_now: &DateTime<Utc>,
    ) -> Result<Self, Error> {
        let pacific_now = utc_now.with_timezone(&Pacific);

        let line_schedule: NextGameSchedule =
            serde_json::from_str(&linescore_response_string).context("line_schedule")?;

        let next = if line_schedule.game_today(utc_now) {
            let first = String::from("1st");
            let no_time = String::from("00:00");
            let game_date = &line_schedule.dates[0];
            let game = &game_date.games[0];
            let linescore = game.linescore.as_ref().expect("linescore");
            let game_date_pacific = game.game_date.with_timezone(&Pacific);
            let opponent_name = opponent_name(&game.teams, team_id);
            let bottom;
            let sleep = sleep_time(&game_date_pacific, &pacific_now);
            let top = if game.status.is_preview() {
                if game.status.is_pregame() {
                    bottom = "Live".to_string();
                    "Pregame".to_string()
                } else {
                    bottom = format!("Today @ {}", format_date_time(&game_date_pacific));
                    "Next Up".to_string()
                }
            } else if game.status.is_live() {
                bottom = "Live".to_string();
                let intermission_info = linescore
                    .intermission_info
                    .as_ref()
                    .expect("intermission_info");
                if intermission_info.in_intermission {
                    let intermission_time_left = chrono::Duration::seconds(
                        intermission_info.intermission_time_remaining as i64,
                    );
                    let m = intermission_time_left.num_minutes();
                    let s = intermission_time_left.num_seconds() - m * 60;
                    format!(
                        "{} int | {:02}:{:02}",
                        linescore.current_period_ordinal.as_ref().unwrap_or(&first),
                        m,
                        s
                    )
                } else {
                    format!(
                        "{} | {}",
                        linescore.current_period_ordinal.as_ref().unwrap_or(&first),
                        linescore
                            .current_period_time_remaining
                            .as_ref()
                            .unwrap_or(&no_time)
                    )
                }
            } else {
                bottom = "".to_string();
                "Final".to_string()
            };
            NextUp {
                bottom,
                middle: opponent_name,
                top: top.into(),
                time: format_date_time(&pacific_now),
                sleep,
            }
        } else {
            let schedule: Response =
                serde_json::from_str(&next_response_string).context("next schedule")?;
            let team = &schedule.teams[0];
            let next_game_schedule = &team.next_game_schedule;
            let game_date = &next_game_schedule.dates[0];
            let game = &game_date.games[0];

            let game_date_pacific = game.game_date.with_timezone(&Pacific);
            let pacific_now = utc_now.with_timezone(&Pacific);

            let sleep = sleep_time(&game_date_pacific, &pacific_now);

            let opponent_name = opponent_name(&game.teams, team_id);

            let date_str = format_game_time_relative(&game_date_pacific, &pacific_now);

            NextUp {
                bottom: date_str,
                middle: opponent_name,
                top: "Next Up".to_string(),
                time: format_date_time(&pacific_now),
                sleep,
            }
        };
        Ok(next)
    }
}

async fn get_next_up(_req: tide::Request<()>) -> tide::Result {
    let opt = Opt::from_args();
    let team_id = opt.team.unwrap_or(SHARKS_ID);
    let utc_now: DateTime<Utc> = Utc::now();

    let next_response_string = if let Some(next) = opt.next.as_ref() {
        fs::read_to_string(next)?
    } else {
        let mut next_response = surf::get(format!(
            "https://statsapi.web.nhl.com/api/v1/teams/{}?expand=team.schedule.next",
            team_id
        ))
        .await?;
        let next_response_string = next_response.body_string().await?;
        next_response_string
    };

    let linescore_response_string = if let Some(line) = opt.line.as_ref() {
        fs::read_to_string(line)?
    } else {
        let mut line_response = surf::get(format!(
            "https://statsapi.web.nhl.com/api/v1/schedule?expand=schedule.linescore&teamId={}",
            team_id
        ))
        .await?;
        let line_response_string = line_response.body_string().await?;
        line_response_string
    };

    let next = NextUp::new(
        &linescore_response_string,
        &next_response_string,
        team_id,
        &utc_now,
    )?;

    let next_json = serde_json::to_string(&next)?;

    let response = tide::Response::builder(tide::StatusCode::Ok)
        .body(next_json)
        .content_type(http_types::mime::JSON)
        .build();

    Ok(response)
}

async fn redirect_root(_request: tide::Request<()>) -> tide::Result {
    Ok(tide::Redirect::new("/next").into())
}

#[async_std::main]
async fn main() -> Result<(), Error> {
    let opt = Opt::from_args();

    if opt.verbose {
        env::set_var("RUST_LOG", "info");
    }

    let utc: DateTime<Utc> = Utc::now(); // e.g. `2014-11-28T12:45:59.324310806Z`
    let local: DateTime<Local> = Local::now(); // e.g. `2014-11-28T21:45:59.324310806+09:00`

    info!("utc {}", utc);
    info!("local {}", local);
    info!("pacific {}", utc.with_timezone(&Pacific));

    let default_port = String::from("8080");
    let port = env::var("PORT").unwrap_or(default_port);
    info!("starting on port {}", port);

    tide::log::start();

    let mut app = tide::new();
    app.at("/").get(redirect_root);
    app.at("/next").get(get_next_up);
    app.listen(format!("0.0.0.0:{}", port)).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    const EMPTY_LINESCORE: &str = r#"{"totalItems": 0, "dates": []}"#;
    const NEXT_TEXT: &str = include_str!("../data/next.json");
    const NJD_BEFORE_TEXT: &str = include_str!("../data/NJD_before.json");
    const NJD_BEFORE_LINESCORE_TEXT: &str = include_str!("../data/NJD_before_linescore.json");
    const NJD_PREGAME_LINESCORE_TEXT: &str = include_str!("../data/NJD_pregame_linescore.json");
    const NJD_DURING_01_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_01_linescore.json");
    const NJD_DURING_02_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_02_linescore.json");
    const NJD_DURING_03_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_03_linescore.json");
    const NJD_DURING_04_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_04_linescore.json");
    const NJD_DURING_05_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_05_linescore.json");
    const NJD_DURING_06_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_06_linescore.json");
    const NJD_DURING_07_LINESCORE_TEXT: &str = include_str!("../data/NJD_during_07_linescore.json");
    const NJD_AFTER_LINESCORE_TEXT: &str = include_str!("../data/NJD_after_linescore.json");
    const SJS_INT_LINESCORE_TEXT: &str = include_str!("../data/sjs_int_linescore.json");
    const SJS_AFTER_LINESCORE_TEXT: &str = include_str!("../data/sjs_after_linescore.json");
    const SJS_AFTER_TEXT: &str = include_str!("../data/sjs_after.json");

    #[test]
    fn test_next() {
        let schedule: Response = serde_json::from_str(&NEXT_TEXT).expect("from_str");
        assert_eq!(1, schedule.teams.len());
        let team = &schedule.teams[0];
        assert_eq!(SHARKS_ID, team.id);
        let next_game_schedule = &team.next_game_schedule;
        assert_eq!(1, next_game_schedule.total_items);
        assert_eq!(1, next_game_schedule.dates.len());
        let game_date = &next_game_schedule.dates[0];
        assert_eq!(game_date.date, "2021-03-20");
    }

    fn test_engine_with_team(
        today: &DateTime<Utc>,
        team_id: usize,
        linescore_response_string: &str,
        next_response_string: &str,
        top: &str,
        middle: &str,
        bottom: &str,
    ) {
        let next_up = NextUp::new(
            linescore_response_string,
            next_response_string,
            team_id,
            today,
        )
        .expect("test_engine_with_team: next up to succeed");
        assert_eq!(next_up.top, top);
        assert_eq!(next_up.middle, middle);
        assert_eq!(next_up.bottom, bottom);
    }

    fn test_engine(
        today: &DateTime<Utc>,
        linescore_response_string: &str,
        next_response_string: &str,
        top: &str,
        middle: &str,
        bottom: &str,
    ) {
        test_engine_with_team(
            today,
            1,
            linescore_response_string,
            next_response_string,
            top,
            middle,
            bottom,
        );
    }

    #[test]
    fn test_njd_before() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-19T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            EMPTY_LINESCORE,
            NJD_BEFORE_TEXT,
            "Next Up",
            "@ Pittsburgh Penguins",
            "Mar 21 @ 10:00AM",
        );
    }

    #[test]
    fn test_njd_before_linescore() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_BEFORE_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "Next Up",
            "@ Pittsburgh Penguins",
            "Today @ 10:00AM",
        );
    }

    #[test]
    fn test_njd_before_two_days() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-20T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            EMPTY_LINESCORE,
            NJD_BEFORE_TEXT,
            "Next Up",
            "@ Pittsburgh Penguins",
            "Mar 21 @ 10:00AM",
        );
    }

    #[test]
    fn test_njd_pregame() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_PREGAME_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "Pregame",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_01() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_01_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "1st int | 12:41",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_02() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_02_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "1st int | 00:00",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_03() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_03_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "2nd | 18:32",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_04() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_04_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "2nd | 03:50",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_05() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_05_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "3rd | 08:20",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_06() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_06_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "3rd | 00:26",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_during_07() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_DURING_07_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "OT | 02:43",
            "@ Pittsburgh Penguins",
            "Live",
        );
    }

    #[test]
    fn test_njd_after() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-21T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine(
            &today,
            NJD_AFTER_LINESCORE_TEXT,
            NJD_BEFORE_TEXT,
            "Final",
            "@ Pittsburgh Penguins",
            "",
        );
    }

    #[test]
    fn test_sjs_int() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-03-29T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine_with_team(
            &today,
            28,
            SJS_INT_LINESCORE_TEXT,
            NEXT_TEXT,
            "1st int | 08:46",
            "vs Minnesota Wild",
            "Live",
        );
    }

    #[test]
    fn test_sjs_after_linescore() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-04-02T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine_with_team(
            &today,
            28,
            SJS_AFTER_LINESCORE_TEXT,
            SJS_AFTER_TEXT,
            "Final",
            "@ Los Angeles Kings",
            "",
        );
    }
}
