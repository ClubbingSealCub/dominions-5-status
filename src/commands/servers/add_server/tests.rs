use super::*;

use std::io;
use model::GameData;

#[test]
fn should_return_error_on_no_connection() {
    struct TestServerConnection;
    impl ServerConnection for TestServerConnection {
        fn get_game_data(_server_address: &str) -> io::Result<GameData> {
            Err(io::Error::from_raw_os_error(-1))
        }
    }

    let result = add_server_helper::<TestServerConnection>("", "", &DbConnection::noop());
    assert!(result.is_err());
}

#[test]
fn should_insert_started_server_into_db() {
    static TEST_ADDRESS: &'static str = "address:1234";
    static TEST_ALIAS: &'static str = ":butts:";

    lazy_static! {
        static ref TEST_GAMEDATA: GameData = GameData {
            game_name: TEST_ALIAS.to_owned(),
            nations: Vec::new(),
            turn: 32,
            turn_timer: 3 * 360,
        };
    }

    struct TestServerConnection;
    impl ServerConnection for TestServerConnection {
        fn get_game_data(server_address: &str) -> io::Result<GameData> {
            if server_address == TEST_ADDRESS {
                Ok(TEST_GAMEDATA.clone())
            } else {
                Err(io::Error::from_raw_os_error(-1))
            }
        }
    }

    let db_conn = DbConnection::test();
    let insert_result = add_server_helper::<TestServerConnection>(&TEST_ADDRESS, &TEST_ALIAS, &db_conn);
    assert!(insert_result.is_ok());

    let fetch_result = db_conn.game_for_alias(&TEST_ALIAS);
    assert!(fetch_result.is_ok());

    let expected_result = GameServer {
        alias: TEST_ALIAS.to_owned(),
        state: GameServerState::StartedState(
            StartedState {
              last_seen_turn: TEST_GAMEDATA.turn,
              address: TEST_ADDRESS.to_owned(),
          },
            None,
        ),
    };

    assert_eq!(fetch_result.unwrap(), expected_result);
}