use crate::server::ServerConnection;

use serenity::framework::standard::CommandError;

use crate::db::DbConnection;
use crate::model::enums::{Era, NationStatus, Nations, SubmissionStatus};
use crate::model::{GameData, GameServerState, LobbyState, Nation, Player, StartedState};
use crate::snek::SnekGameStatus;
use log::*;
use serenity::model::id::UserId;
use std::cmp::max;
use std::cmp::Ordering;
use std::collections::HashMap;

/// We cache the call to the server (both the game itself and the snek api)
/// but NOT the db call
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CacheEntry {
    pub game_data: GameData,
    pub option_snek_state: Option<SnekGameStatus>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct GameDetails {
    pub alias: String,
    pub owner: Option<UserId>,
    pub description: Option<String>,
    pub nations: NationDetails,
    pub cache_entry: Option<CacheEntry>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum NationDetails {
    Lobby(LobbyDetails),
    Started(StartedDetails),
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct StartedDetails {
    pub address: String,
    pub game_name: String,
    pub state: StartedStateDetails,
}

pub fn get_nation_string(option_snek_state: &Option<SnekGameStatus>, nation_id: u32) -> String {
    let snek_nation_details = option_snek_state
        .as_ref()
        .and_then(|snek_details| snek_details.nations.get(&nation_id));
    match snek_nation_details {
        Some(snek_nation) => snek_nation.name.clone(),
        None => {
            let &(nation_name, _) = Nations::get_nation_desc(nation_id);
            nation_name.to_owned()
        }
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum StartedStateDetails {
    Playing(PlayingState),
    Uploading(UploadingState),
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct UploadingState {
    pub uploading_players: Vec<UploadingPlayer>,
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct PlayingState {
    pub players: Vec<PotentialPlayer>,
    pub turn: u32,
    pub mins_remaining: i32,
    pub hours_remaining: i32,
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum PotentialPlayer {
    RegisteredOnly(UserId, u32, String),
    RegisteredAndGame(UserId, PlayerDetails),
    GameOnly(PlayerDetails),
}
impl PotentialPlayer {
    pub fn nation_name(&self) -> &String {
        match &self {
            PotentialPlayer::RegisteredOnly(_, _, nation_name) => nation_name,
            PotentialPlayer::RegisteredAndGame(_, details) => &details.nation_name,
            PotentialPlayer::GameOnly(details) => &details.nation_name,
        }
    }
    pub fn nation_id(&self) -> u32 {
        match &self {
            PotentialPlayer::RegisteredOnly(_, nation_id, _) => *nation_id,
            PotentialPlayer::RegisteredAndGame(_, details) => details.nation_id,
            PotentialPlayer::GameOnly(details) => details.nation_id,
        }
    }
    pub fn option_player_id(&self) -> Option<&UserId> {
        match &self {
            PotentialPlayer::RegisteredOnly(player_id, _, _) => Some(player_id),
            PotentialPlayer::RegisteredAndGame(player_id, _) => Some(player_id),
            PotentialPlayer::GameOnly(_) => None,
        }
    }
}
impl PartialOrd<Self> for PotentialPlayer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PotentialPlayer {
    fn cmp(&self, other: &Self) -> Ordering {
        self.nation_name().cmp(&other.nation_name())
    }
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct PlayerDetails {
    pub nation_id: u32,
    pub nation_name: String,
    pub submitted: SubmissionStatus,
    pub player_status: NationStatus,
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct UploadingPlayer {
    pub potential_player: PotentialPlayer,
    pub uploaded: bool,
}
impl UploadingPlayer {
    pub fn nation_name(&self) -> &String {
        self.potential_player.nation_name()
    }
    pub fn nation_id(&self) -> u32 {
        self.potential_player.nation_id()
    }
    pub fn option_player_id(&self) -> Option<&UserId> {
        self.potential_player.option_player_id()
    }
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct LobbyDetails {
    pub players: Vec<LobbyPlayer>,
    pub era: Option<Era>,
    pub remaining_slots: u32,
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct LobbyPlayer {
    pub player_id: UserId,
    pub nation_id: u32,
    pub nation_name: String,
}

pub fn get_details_for_alias<C: ServerConnection>(
    db_conn: &DbConnection,
    alias: &str,
) -> Result<GameDetails, CommandError> {
    let server = db_conn.game_for_alias(&alias)?;
    info!("got server details");

    let details = match server.state {
        GameServerState::Lobby(ref lobby_state) => lobby_details(db_conn, lobby_state, &alias)?,
        GameServerState::StartedState(ref started_state, ref option_lobby_state) => {
            started_details::<C>(db_conn, started_state, option_lobby_state.as_ref(), &alias)?
        }
    };

    Ok(details)
}

pub fn lobby_details(
    db_conn: &DbConnection,
    lobby_state: &LobbyState,
    alias: &str,
) -> Result<GameDetails, CommandError> {
    let players_nations = db_conn.players_with_nations_for_game_alias(&alias)?;

    let mut player_nation_details: Vec<LobbyPlayer> = players_nations
        .into_iter()
        .map(|(player, nation_id)| -> LobbyPlayer {
            let &(nation_name, _) = Nations::get_nation_desc(nation_id);
            LobbyPlayer {
                player_id: player.discord_user_id,
                nation_id,
                nation_name: nation_name.to_owned(),
            }
        })
        .collect();
    player_nation_details.sort_by(|n1, n2| n1.nation_name.cmp(&n2.nation_name));

    let remaining_slots = max(
        0,
        (lobby_state.player_count - player_nation_details.len() as i32) as u32,
    );

    let lobby_details = LobbyDetails {
        players: player_nation_details,
        era: Some(lobby_state.era),
        remaining_slots,
    };

    Ok(GameDetails {
        alias: alias.to_owned(),
        owner: Some(lobby_state.owner),
        description: lobby_state.description.clone(),
        nations: NationDetails::Lobby(lobby_details),
        cache_entry: None, // lobbies have no cache entry
    })
}

fn started_details<C: ServerConnection>(
    db_conn: &DbConnection,
    started_state: &StartedState,
    option_lobby_state: Option<&LobbyState>,
    alias: &str,
) -> Result<GameDetails, CommandError> {
    let server_address = &started_state.address;
    let game_data = C::get_game_data(&server_address)?;
    let option_snek_details = C::get_snek_data(server_address)?;

    started_details_from_server(
        db_conn,
        started_state,
        option_lobby_state,
        alias,
        game_data,
        option_snek_details,
    )
}

pub fn started_details_from_server(
    db_conn: &DbConnection,
    started_state: &StartedState,
    option_lobby_state: Option<&LobbyState>,
    alias: &str,
    game_data: GameData,
    option_snek_details: Option<SnekGameStatus>,
) -> Result<GameDetails, CommandError> {
    let id_player_nations = db_conn.players_with_nations_for_game_alias(&alias)?;
    let player_details =
        join_players_with_nations(&game_data.nations, &id_player_nations, &option_snek_details)?;

    let state_details = if game_data.turn < 0 {
        let uploaded_players_detail: Vec<UploadingPlayer> = player_details
            .into_iter()
            .map(|potential_player_detail| {
                match potential_player_detail {
                    potential_player @ PotentialPlayer::GameOnly(_) => {
                        UploadingPlayer {
                            potential_player,
                            uploaded: true, // all players we can see have uploaded
                        }
                    }
                    potential_player @ PotentialPlayer::RegisteredAndGame(_, _) => {
                        UploadingPlayer {
                            potential_player,
                            uploaded: true, // all players we can see have uploaded
                        }
                    }
                    potential_player @ PotentialPlayer::RegisteredOnly(_, _, _) => {
                        UploadingPlayer {
                            potential_player,
                            uploaded: false, // all players we can't see have not uploaded
                        }
                    }
                }
            })
            .collect();

        StartedStateDetails::Uploading(UploadingState {
            uploading_players: uploaded_players_detail,
        })
    } else {
        let total_mins_remaining = game_data.turn_timer / (1000 * 60);
        let hours_remaining = total_mins_remaining / 60;
        let mins_remaining = total_mins_remaining - hours_remaining * 60;
        StartedStateDetails::Playing(PlayingState {
            players: player_details,
            mins_remaining,
            hours_remaining,
            turn: game_data.turn as u32, // game_data >= 0 checked above
        })
    };

    let started_details = StartedDetails {
        address: started_state.address.clone(),
        game_name: game_data.game_name.clone(),
        state: state_details,
    };

    Ok(GameDetails {
        alias: alias.to_owned(),
        owner: option_lobby_state.map(|lobby_state| lobby_state.owner.clone()),
        description: option_lobby_state.and_then(|lobby_state| lobby_state.description.clone()),
        nations: NationDetails::Started(started_details),
        cache_entry: Some(CacheEntry {
            game_data: game_data.clone(),
            option_snek_state: option_snek_details.clone(),
        }),
    })
}

fn join_players_with_nations(
    nations: &Vec<Nation>,
    players_nations: &Vec<(Player, u32)>,
    option_snek_details: &Option<SnekGameStatus>,
) -> Result<Vec<PotentialPlayer>, CommandError> {
    let mut potential_players = vec![];

    let mut players_by_nation_id = HashMap::new();
    for (player, nation_id) in players_nations {
        players_by_nation_id.insert(*nation_id, player);
    }
    for nation in nations {
        match players_by_nation_id.remove(&nation.id) {
            // Lobby and game
            Some(player) => {
                let player_details = PlayerDetails {
                    nation_id: nation.id,
                    nation_name: get_nation_string(option_snek_details, nation.id),
                    submitted: nation.submitted,
                    player_status: nation.status,
                };
                potential_players.push(PotentialPlayer::RegisteredAndGame(
                    player.discord_user_id,
                    player_details,
                ))
            }
            // Game only
            None => potential_players.push(PotentialPlayer::GameOnly(PlayerDetails {
                nation_id: nation.id,
                nation_name: get_nation_string(option_snek_details, nation.id),
                submitted: nation.submitted,
                player_status: nation.status,
            })),
        }
    }
    // Lobby only
    for (nation_id, player) in players_by_nation_id {
        let &(nation_name, _) = Nations::get_nation_desc(nation_id);
        potential_players.push(PotentialPlayer::RegisteredOnly(
            player.discord_user_id,
            nation_id,
            nation_name.to_owned(),
        ));
    }
    potential_players.sort_unstable();
    Ok(potential_players)
}
