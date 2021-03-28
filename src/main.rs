#![allow(unused)]
use anyhow::{Error, Result};
use chrono::{Date, DateTime, FixedOffset, Local, Timelike, Utc};
use chrono_tz::US::Pacific;
use log::info;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self},
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use tide::prelude::*;

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
    current_period_ordinal: String,
    current_period_time_remaining: String,
    intermission_info: IntermissionInfo,
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

async fn get_next_up(mut req: tide::Request<()>) -> tide::Result {
    let opt = Opt::from_args();
    let team_id = opt.team.unwrap_or(SHARKS_ID);
    let today = chrono::offset::Local::today();
    let tomorrow = today.succ();

    let utc_now: DateTime<Utc> = Utc::now();
    let pacific_now = utc_now.with_timezone(&Pacific);

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

    let line_schedule: NextGameSchedule = serde_json::from_str(&linescore_response_string)?;

    let next = if line_schedule.total_items > 0 {
        let game_date = &line_schedule.dates[0];
        let game = &game_date.games[0];
        let linescore = game.linescore.as_ref().expect("linescore");
        let game_date_pacific = game.game_date.with_timezone(&Pacific);
        let opponent_name = opponent_name(&game.teams, team_id);
        let mut bottom = format!("{}", format_date_time(&game_date_pacific));
        let top = if game.status.is_preview() {
            if game.status.is_pregame() {
                "Pregame".to_string()
            } else {
                "Today".to_string()
            }
        } else if game.status.is_live() {
            if linescore.intermission_info.in_intermission {
                let intermission_time_left = chrono::Duration::seconds(
                    linescore.intermission_info.intermission_time_remaining as i64,
                );
                let m = intermission_time_left.num_minutes();
                let s = intermission_time_left.num_seconds() - m * 60;
                format!(
                    "{} intermission {:02}:{:02}",
                    linescore.current_period_ordinal, m, s
                )
            } else {
                format!(
                    "{} {}",
                    linescore.current_period_ordinal, linescore.current_period_time_remaining
                )
            }
        } else {
            bottom = "".to_string();
            "Final".to_string()
        };
        let ht = chrono_humanize::HumanTime::from(game_date_pacific);
        ht.to_text_en(
            chrono_humanize::Accuracy::Precise,
            chrono_humanize::Tense::Future,
        );
        NextUp {
            bottom,
            middle: opponent_name,
            top: top.into(),
            time: format_date_time(&pacific_now),
        }
    } else {
        let schedule: Response = serde_json::from_str(&next_response_string)?;
        let team = &schedule.teams[0];
        let next_game_schedule = &team.next_game_schedule;
        let game_date = &next_game_schedule.dates[0];
        let game = &game_date.games[0];

        let game_date_pacific = game.game_date.with_timezone(&Pacific);

        info!("game = {:?}", game);

        let opponent_name = opponent_name(&game.teams, team_id);

        let date_str = if game_date_pacific.date() == tomorrow {
            String::from("Tomorrow")
        } else {
            let ht = chrono_humanize::HumanTime::from(game_date_pacific);
            ht.to_text_en(
                chrono_humanize::Accuracy::Rough,
                chrono_humanize::Tense::Future,
            )
        };

        NextUp {
            bottom: date_str,
            middle: opponent_name,
            top: "Next Up".to_string(),
            time: format_date_time(&pacific_now),
        }
    };

    let next_json = serde_json::to_string(&next)?;

    let mut response = tide::Response::builder(tide::StatusCode::Ok)
        .body(next_json)
        .content_type(http_types::mime::JSON)
        .build();

    Ok(response)
}

async fn redirect_root(request: tide::Request<()>) -> tide::Result {
    Ok(tide::Redirect::new("/next").into())
}

#[async_std::main]
async fn main() -> Result<(), Error> {
    let opt = Opt::from_args();

    if opt.verbose {
        env::set_var("RUST_LOG", "info");
    }

    pretty_env_logger::init();

    let utc: DateTime<Utc> = Utc::now(); // e.g. `2014-11-28T12:45:59.324310806Z`
    let local: DateTime<Local> = Local::now(); // e.g. `2014-11-28T21:45:59.324310806+09:00`

    info!("utc {}", utc);
    info!("local {}", local);
    info!("pacific {}", utc.with_timezone(&Pacific));

    let default_port = String::from("8080");
    let port = env::var("PORT").unwrap_or(default_port);
    info!("starting on port {}", port);

    let mut app = tide::new();
    app.at("/").get(redirect_root);
    app.at("/next").get(get_next_up);
    app.listen(format!("0.0.0.0:{}", port)).await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    const NEXT_TEXT: &str = include_str!("../data/next.json");

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
}
