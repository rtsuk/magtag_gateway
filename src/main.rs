#![allow(unused)]
use anyhow::{Error, Result};
use chrono::{Date, DateTime, FixedOffset, Local, Timelike, Utc};
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
    line: bool,

    #[structopt(short, long)]
    file: Option<PathBuf>,
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
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub game_date: DateTime<Utc>,
    pub teams: Teams,
    pub status: Status,
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

fn parse_file(opt: &Opt) -> Result<(), Error> {
    if let Some(file) = opt.file.as_ref() {
        let schedule_string = fs::read_to_string(file)?;
        if opt.line {
            let schedule: NextGameSchedule = serde_json::from_str(&schedule_string)?;
            dbg!(schedule);
        } else {
            let schedule: Response = serde_json::from_str(&schedule_string)?;
            dbg!(schedule);
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
struct NextUp {
    top: String,
    middle: String,
    bottom: String,
}

async fn get_next_up(mut req: tide::Request<()>) -> tide::Result {
    let mut next_response =
        surf::get("https://statsapi.web.nhl.com/api/v1/teams/28?expand=team.schedule.next").await?;
    let next_response_string = next_response.body_string().await?;

    let schedule: Response = serde_json::from_str(&next_response_string)?;
    let team = &schedule.teams[0];
    let next_game_schedule = &team.next_game_schedule;
    let game_date = &next_game_schedule.dates[0];
    let game = &game_date.games[0];

    info!("game = {:?}", game);

    let opponent_name = if game.teams.home.team.id == SHARKS_ID {
        format!("vs {}", game.teams.away.team.name)
    } else {
        format!("@ {}", game.teams.home.team.name)
    };

    let tomorrow = chrono::offset::Local::today().succ();
    let date_str = if game.game_date.with_timezone(&Local).date()== tomorrow {
        String::from("Tomorrow")
    } else {
        let ht = chrono_humanize::HumanTime::from(game.game_date);
        ht.to_text_en(
            chrono_humanize::Accuracy::Rough,
            chrono_humanize::Tense::Future,
        )
    };

    let next = NextUp {
        bottom: date_str,
        middle: opponent_name,
        top: "Next Up".to_string(),
    };

    let next_json = serde_json::to_string(&next)?;

    let mut response = tide::Response::builder(tide::StatusCode::Ok)
        .body(next_json)
        .content_type(http_types::mime::JSON)
        .build();

    Ok(response)
}

#[async_std::main]
async fn main() -> Result<(), Error> {
    let opt = Opt::from_args();

    if opt.verbose {
        env::set_var("RUST_LOG", "info");
    }

    pretty_env_logger::init();

    let default_port = String::from("8080");
    let port = env::var("PORT").unwrap_or(default_port);
    info!("starting on port {}", port);

    if opt.file.is_some() {
        parse_file(&opt)?;
    } else {
        let mut app = tide::new();
        app.at("/next").get(get_next_up);
        app.listen(format!("0.0.0.0:{}", port)).await?;
    }

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
