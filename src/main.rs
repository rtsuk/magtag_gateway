use anyhow::{Context, Error, Result};
use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::US::Pacific;
use log::info;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    fs::{self},
    path::PathBuf,
};
use structopt::StructOpt;

const ONE_MINUTE_IN_SECONDS: i64 = 60;
const ONE_HOUR_IN_SECONDS: i64 = 60 * ONE_MINUTE_IN_SECONDS;
const TWO_HOURS_IN_SECONDS: i64 = 2 * ONE_HOUR_IN_SECONDS;
const TWENTY_MINUTES_IN_SECONDS: i64 = 20 * ONE_MINUTE_IN_SECONDS;
const EVENTS_TEXT: &str = include_str!("../data/events.toml");

const CUDA_NEXT_UP: &str = "Cuda Next Up";
const SHARKS_NEXT_UP: &str = "Sharks Next Up";

pub static TEAM_NICKNAMES: Lazy<HashMap<usize, &'static str>> = Lazy::new(|| {
    [
        (1, "Devils"),
        (2, "Islanders"),
        (3, "Rangers"),
        (4, "Flyers"),
        (5, "Penguins"),
        (6, "Bruins"),
        (7, "Sabres"),
        (8, "Canadiens"),
        (9, "Senators"),
        (10, "Leafs"),
        (12, "Canes"),
        (13, "Panthers"),
        (14, "Lightning"),
        (15, "Capitals"),
        (16, "Blackhawks"),
        (17, "Wings"),
        (18, "Predators"),
        (19, "Blues"),
        (20, "Flames"),
        (21, "Avalanche"),
        (22, "Oilers"),
        (23, "Canucks"),
        (24, "Ducks"),
        (25, "Stars"),
        (26, "Kings"),
        (28, "Sharks"),
        (29, "Jackets"),
        (30, "Wild"),
        (52, "Jets"),
        (54, "Coyotes"),
        (55, "Knights"),
        (56, "Kraken"),
    ]
    .iter()
    .cloned()
    .collect()
});

#[derive(Serialize, Deserialize, Debug, PartialOrd, Ord, PartialEq, Eq)]
struct Event {
    text: String,
    date: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
struct EventList {
    events: Vec<Event>,
}

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

    fn is_tbd(&self) -> bool {
        self.detailed_state == "Scheduled (Time TBD)"
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
    pub game_pk: usize,
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
    pub next_game_schedule: Option<NextGameSchedule>,
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
    date: DateTime<Utc>,
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
    is_tdb: bool,
) -> String {
    let date_pacific = date_time.with_timezone(&Pacific);
    let pacific_now = utc_now.with_timezone(&Pacific);
    if date_pacific.date() == pacific_now.date() {
        if is_tdb {
            "Today".to_string()
        } else {
            date_time.format("Today @ %-I:%M%p").to_string()
        }
    } else {
        if is_tdb {
            date_time.format("%b %-d").to_string()
        } else {
            date_time.format("%b %-d @ %-I:%M%p").to_string()
        }
    }
}

fn sleep_time(date_time: &DateTime<chrono_tz::Tz>, utc_now: &DateTime<chrono_tz::Tz>) -> i64 {
    let duration_until_game = *date_time - *utc_now;
    let duration_until_game_seconds = duration_until_game.num_seconds();
    if duration_until_game_seconds < 0 {
        TWO_HOURS_IN_SECONDS
    } else if duration_until_game_seconds > TWENTY_MINUTES_IN_SECONDS {
        (duration_until_game_seconds - TWENTY_MINUTES_IN_SECONDS).min(TWO_HOURS_IN_SECONDS)
    } else {
        ONE_MINUTE_IN_SECONDS
    }
}

#[derive(Debug, PartialEq)]
struct PlayoffGameNumber {
    round: usize,
    matchup: usize,
    game: usize,
}

impl PlayoffGameNumber {
    #[allow(unused)]
    fn new(round: usize, matchup: usize, game: usize) -> Self {
        Self {
            round,
            matchup,
            game,
        }
    }

    fn parse(game_number: usize) -> Self {
        let game_number = game_number % 10_000;
        let round = game_number / 100;
        let matchup = (game_number % 100) / 10;
        let game = game_number % 10;
        Self {
            round,
            matchup,
            game,
        }
    }
}

#[derive(Debug, PartialEq)]
enum GameType {
    Preseason(usize),
    Regular(usize),
    Playoff(PlayoffGameNumber),
}

impl GameType {
    fn parse(game_id: usize) -> Self {
        let game_number = game_id % 100_000;
        let game_type = game_number / 10_000;
        match game_type {
            1 => GameType::Preseason(game_number),
            3 => GameType::Playoff(PlayoffGameNumber::parse(game_number)),
            _ => GameType::Regular(game_number),
        }
    }
}

#[derive(Debug, PartialEq)]
struct GameId {
    season: usize,
    game_type: GameType,
}

fn decode_game_id(game_id: usize) -> Option<GameId> {
    let season = game_id / 1_000_000;

    Some(GameId {
        season,
        game_type: GameType::parse(game_id),
    })
}

fn formatted_next_up(team: &str, game_id: usize) -> String {
    let default_value = format!("{} Next Up", team);
    if let Some(game_id) = decode_game_id(game_id) {
        match game_id.game_type {
            GameType::Playoff(pgn) => format!("{} - Game {}", team, pgn.game),
            _ => default_value,
        }
    } else {
        default_value
    }
}

impl Default for NextUp {
    fn default() -> Self {
        let utc_now: DateTime<Utc> = Utc::now();
        let pacific_now = utc_now.with_timezone(&Pacific);
        let sleep = 900;
        Self {
            bottom: "".to_string(),
            middle: "No Games".to_string(),
            top: "No Team Name".to_string(),
            time: format_date_time(&pacific_now),
            sleep,
            date: utc_now,
        }
    }
}

impl NextUp {
    fn new(
        nickname: &str,
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
                    formatted_next_up(nickname, game.game_pk)
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
                        "{} int|{}:{:02}",
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
                date: game.game_date,
            }
        } else {
            let schedule: Response =
                serde_json::from_str(&next_response_string).context("next schedule")?;
            let team = &schedule.teams[0];
            let next_game_schedule = &team.next_game_schedule;
            let pacific_now = utc_now.with_timezone(&Pacific);
            if let Some(next_game_schedule) = next_game_schedule {
                let game_date = &next_game_schedule.dates[0];
                let game = &game_date.games[0];

                let game_date_pacific = game.game_date.with_timezone(&Pacific);

                let sleep = sleep_time(&game_date_pacific, &pacific_now);

                let opponent_name = opponent_name(&game.teams, team_id);

                let date_str = format_game_time_relative(
                    &game_date_pacific,
                    &pacific_now,
                    game.status.is_tbd(),
                );

                NextUp {
                    bottom: date_str,
                    middle: opponent_name,
                    top: formatted_next_up(nickname, game.game_pk),
                    time: format_date_time(&pacific_now),
                    sleep,
                    date: game.game_date,
                }
            } else {
                NextUp {
                    top: format!("{} Next Up", nickname),
                    ..NextUp::default()
                }
            }
        };
        Ok(next)
    }

    fn new_event(utc_now: &DateTime<Utc>) -> Result<Self, Error> {
        let pacific_now = utc_now.with_timezone(&Pacific);
        let mut events: EventList = toml::from_str(EVENTS_TEXT).expect("events");
        events
            .events
            .sort_by(|a, b| a.date.partial_cmp(&b.date).expect("partial_cmp"));
        let event = events.events.iter().find(|event| event.date > *utc_now);
        if let Some(event) = event {
            let event_date_pacific = event.date.with_timezone(&Pacific);
            let sleep = sleep_time(&event_date_pacific, &pacific_now);
            let date_str = format_game_time_relative(&event_date_pacific, &pacific_now, false);
            Ok(Self {
                top: SHARKS_NEXT_UP.to_string(),
                middle: event.text.clone(),
                bottom: date_str,
                time: format_date_time(&pacific_now),
                sleep,
                date: event.date,
            })
        } else {
            Ok(Self {
                top: SHARKS_NEXT_UP.to_string(),
                ..Self::default()
            })
        }
    }

    fn new_barracuda_event(utc_now: &DateTime<Utc>, games: Vec<AhlGame>) -> Result<Self, Error> {
        let pacific_now = utc_now.with_timezone(&Pacific);
        let maybe_next_game = games.iter().find(|game| game.date > *utc_now);
        if let Some(next_game) = maybe_next_game {
            let event_date_pacific = next_game.date.with_timezone(&Pacific);
            let sleep = sleep_time(&event_date_pacific, &pacific_now);
            let date_str = format_game_time_relative(&event_date_pacific, &pacific_now, false);
            Ok(Self {
                bottom: date_str,
                middle: next_game.opponent_name.clone(),
                top: CUDA_NEXT_UP.to_string(),
                time: format_date_time(&pacific_now),
                sleep,
                date: next_game.date,
            })
        } else {
            Ok(Self {
                top: CUDA_NEXT_UP.to_string(),
                ..Self::default()
            })
        }
    }
}

async fn get_nhl_next_up(team_id: usize) -> Result<NextUp, Error> {
    let nickname = TEAM_NICKNAMES.get(&team_id).unwrap_or_else(|| &"Unknown");
    let opt = Opt::from_args();
    let utc_now: DateTime<Utc> = Utc::now();

    let next_response_string = if let Some(next) = opt.next.as_ref() {
        fs::read_to_string(next)?
    } else {
        let mut next_response = surf::get(format!(
            "https://statsapi.web.nhl.com/api/v1/teams/{}?expand=team.schedule.next",
            team_id
        ))
        .await
        .map_err(anyhow::Error::msg)?;
        let next_response_string = next_response
            .body_string()
            .await
            .map_err(anyhow::Error::msg)?;
        next_response_string
    };

    let linescore_response_string = if let Some(line) = opt.line.as_ref() {
        fs::read_to_string(line)?
    } else {
        let mut line_response = surf::get(format!(
            "https://statsapi.web.nhl.com/api/v1/schedule?expand=schedule.linescore&teamId={}",
            team_id
        ))
        .await
        .map_err(anyhow::Error::msg)?;
        let line_response_string = line_response
            .body_string()
            .await
            .map_err(anyhow::Error::msg)?;
        line_response_string
    };

    Ok(NextUp::new(
        nickname,
        &linescore_response_string,
        &next_response_string,
        team_id,
        &utc_now,
    )?)
}

async fn get_next_up(req: tide::Request<()>) -> tide::Result {
    let opt = Opt::from_args();
    let team_id_param = req
        .param("team")
        .ok()
        .and_then(|team_id_str| team_id_str.parse::<usize>().ok());
    let team_id = team_id_param.unwrap_or_else(|| opt.team.unwrap_or(SHARKS_ID));
    let next = get_nhl_next_up(team_id).await.ok().unwrap_or_default();
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

async fn get_events(_req: tide::Request<()>) -> tide::Result {
    let utc_now: DateTime<Utc> = Utc::now();

    let next = NextUp::new_event(&utc_now)?;

    let next_json = serde_json::to_string(&next)?;

    let response = tide::Response::builder(tide::StatusCode::Ok)
        .body(next_json)
        .content_type(http_types::mime::JSON)
        .build();

    Ok(response)
}

fn calculate_year(date_text: &str) -> usize {
    match &date_text[5..8] {
        "Jan" | "Feb" | "Mar" | "Apr" => 2023,
        _ => 2022,
    }
}

#[derive(Debug, Clone)]
pub struct AhlGame {
    date: DateTime<Utc>,
    opponent_name: String,
}

pub fn load_games_from_page(page: &str) -> Vec<AhlGame> {
    let document = scraper::Html::parse_document(page);
    let entry_selector = scraper::Selector::parse("div.entry").expect("selector");
    let date_selector = scraper::Selector::parse("div.date-time span.date").expect("date selector");
    let time_selector = scraper::Selector::parse("div.date-time span.time").expect("time selector");
    let away_selector = scraper::Selector::parse("div.home-or-away").expect("home selector");
    let opponent_selector = scraper::Selector::parse("span.team-title").expect("opponent_selector");
    let entries = document.select(&entry_selector);
    entries
        .filter_map(|item| {
            let date_text = item
                .select(&date_selector)
                .map(|element| element.inner_html())
                .collect::<Vec<String>>()
                .join(" ")
                .trim()
                .to_owned();
            if date_text.len() > 0 {
                let time_text = item
                    .select(&time_selector)
                    .map(|element| element.inner_html())
                    .collect::<Vec<String>>()
                    .join(" ")
                    .trim()
                    .to_owned();
                let away_text = item
                    .select(&away_selector)
                    .map(|element| element.inner_html())
                    .collect::<Vec<String>>()
                    .join(" ");
                let away_text_trimmed = away_text.trim();
                let opponent_text = item
                    .select(&opponent_selector)
                    .map(|element| element.inner_html())
                    .collect::<Vec<String>>()
                    .join(" ")
                    .trim()
                    .to_owned();

                if away_text_trimmed.eq_ignore_ascii_case("home") {
                    let time = NaiveTime::parse_from_str(&time_text, "%I:%M%p").expect("time");
                    let year = calculate_year(&date_text);
                    let date_text_with_year = format!("{} {}", date_text, year);
                    let date = NaiveDate::parse_from_str(&date_text_with_year, "%a, %b %d %Y")
                        .expect("date");
                    let naive_dt = date.and_time(time);
                    let tz_aware = Pacific.from_local_datetime(&naive_dt).unwrap();
                    Some(AhlGame {
                        date: tz_aware.with_timezone(&Utc),
                        opponent_name: opponent_text,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

async fn get_barracuda_next_up(_req: tide::Request<()>) -> tide::Result {
    let response = reqwest::blocking::get("https://www.sjbarracuda.com/games")
        .map(|r| r.text().expect("text"));

    let next = if let Ok(response) = response {
        let games = load_games_from_page(&response);
        let utc_now: DateTime<Utc> = Utc::now();
        NextUp::new_barracuda_event(&utc_now, games)?
    } else {
        NextUp {
            top: CUDA_NEXT_UP.to_string(),
            ..NextUp::default()
        }
    };

    let next_json = serde_json::to_string(&next)?;

    let response = tide::Response::builder(tide::StatusCode::Ok)
        .body(next_json)
        .content_type(http_types::mime::JSON)
        .build();

    Ok(response)
}

async fn get_next_up_either(_req: tide::Request<()>) -> tide::Result {
    let response = reqwest::blocking::get("https://www.sjbarracuda.com/games")
        .map(|r| r.text().expect("text"));

    let b_next = if let Ok(response) = response {
        let games = load_games_from_page(&response);
        let utc_now: DateTime<Utc> = Utc::now();
        Some(NextUp::new_barracuda_event(&utc_now, games)?)
    } else {
        None
    };

    let team_id = SHARKS_ID;
    let nhl_next = get_nhl_next_up(team_id).await.ok();

    let next = if b_next.is_none() {
        nhl_next.unwrap_or_default()
    } else if nhl_next.is_none() {
        b_next.unwrap_or_default()
    } else {
        let nhl_next = nhl_next.unwrap();
        let b_next = b_next.unwrap();
        if nhl_next.date < b_next.date {
            nhl_next
        } else {
            b_next
        }
    };

    let next_json = serde_json::to_string(&next)?;

    let response = tide::Response::builder(tide::StatusCode::Ok)
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
    app.at("/next/:team").get(get_next_up);
    app.at("/events").get(get_events);
    app.at("/barracuda").get(get_barracuda_next_up);
    app.at("/either").get(get_next_up_either);
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
    const SJS_DONE_LINESCORE_TEXT: &str = include_str!("../data/sjs_linescore_done.json");
    const SJS_DONE_TEXT: &str = include_str!("../data/sjs_done.json");
    const P1_TEXT: &str = include_str!("../data/p1_playoff.json");
    const BARRACUDA_SCHEDULE_TEXT: &str = include_str!("../data/barracuda.html");

    #[test]
    fn test_next() {
        let schedule: Response = serde_json::from_str(&NEXT_TEXT).expect("from_str");
        assert_eq!(1, schedule.teams.len());
        let team = &schedule.teams[0];
        assert_eq!(SHARKS_ID, team.id);
        let next_game_schedule = &team.next_game_schedule.as_ref().unwrap();
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
            "Sharks Next Up",
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
            "Sharks Next Up",
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
            "Sharks Next Up",
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
            "1st int|12:41",
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
            "1st int|0:00",
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
            "1st int|8:46",
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

    #[test]
    fn test_sjs_done() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-05-14T17:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine_with_team(
            &today,
            28,
            SJS_DONE_LINESCORE_TEXT,
            SJS_DONE_TEXT,
            "Sharks Next Up",
            "No Games",
            "",
        );
    }

    #[test]
    fn test_playoff_game_id() {
        const EDM_FIRST_FIRST: usize = 2020030181;

        let game_id = decode_game_id(EDM_FIRST_FIRST).unwrap();
        assert_eq!(game_id.season, 2020);
        assert_eq!(
            game_id.game_type,
            GameType::Playoff(PlayoffGameNumber::new(1, 8, 1))
        );
    }

    #[test]
    fn test_playoff_one() {
        let today = chrono::DateTime::parse_from_rfc3339("2021-05-14T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        test_engine_with_team(
            &today,
            2,
            EMPTY_LINESCORE,
            P1_TEXT,
            "Next - Game 1",
            "@ Pittsburgh Penguins",
            "May 16 @ 9:00AM",
        );
    }

    #[test]
    fn test_events() {
        let events: EventList = toml::from_str(EVENTS_TEXT).expect("events");
        assert_eq!(events.events.len(), 8);
        assert_eq!(&events.events[1].text, "Sharks365 Season Preview");
        assert_eq!(&events.events[0].text, "Tech CU Arena Fan Reveal");
    }

    #[test]
    fn test_barracuda() {
        let games = load_games_from_page(BARRACUDA_SCHEDULE_TEXT);
        assert_eq!(games.len(), 36);
        assert_eq!(&games[0].opponent_name, "Henderson Silver Knights");
        assert_eq!(&games[2].opponent_name, "Ontario Reign");
        assert_eq!(&games[35].opponent_name, "Colorado Eagles");
    }
}
