//inspired by https://github.com/vilaureu/mirabel_connect_four/blob/main/src/game.rs

use num_traits::FromPrimitive;
use std::fmt::Write;

use surena_game::{
    buf_sizer, create_game_methods, cstr, game_feature_flags, game_methods, move_code, player_id,
    plugin_get_game_methods, semver, Error, ErrorCode::InvalidInput, GameMethods, Metadata, PtrVec,
    Result, StrBuf,
};
use surena_game::{ErrorCode, GameInit};
use three_player_chess::board::*;

pub const GAME_NAME: &str = "ThreePlayerChess\0";
pub const VARIANT_NAME: &str = "Classic\0";
pub const IMPL_NAME: &str = "three_player_chess_cmrs\0";

pub const BUF_SIZER: buf_sizer = buf_sizer {
    options_str: 1,
    state_str: MAX_POSITION_STRING_SIZE + 1,
    player_count: HB_COUNT as u8,
    max_players_to_move: 1,
    // NOTE(cmrs): this is an overestimation, and a very bad guess.
    // But three little birds told me this whole buf sizer thing is gonna die
    // anyways so... good enough
    max_moves: 1024,
    max_results: 1,
    move_str: MAX_MOVE_STRING_SIZE + 1,
    print_str: BOARD_STRING.len() + 1,
    legacy_str: 0,
    max_actions: 0,
    serialization_size: 0,
};

/// Generate [`game_methods`] struct.
fn three_player_chess() -> game_methods {
    let mut features = game_feature_flags::default();
    features.set_print(true);
    // features.set_options(true); // NOTE(cmrs): will come later(tm)

    create_game_methods::<ThreePlayerChessGame>(Metadata {
        game_name: cstr(GAME_NAME),
        variant_name: cstr(VARIANT_NAME),
        impl_name: cstr(IMPL_NAME),
        version: semver {
            major: 0,
            minor: 1,
            patch: 0,
        },
        features,
    })
}

plugin_get_game_methods!(three_player_chess());

/// Struct holding options and game state.
#[derive(PartialEq, Eq, Clone)]
pub struct ThreePlayerChessGame {
    pub options: GameOptions,
    pub board: ThreePlayerChess,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GameOptions {
    // TODO(cmrs): maybe make the eval for the neutral player on checkmate an option.
    // That really only affects engines though.
    // Once there's a system for draw offers / (collective) resignation, we
    // might need to make that configurable here too.
}

impl GameOptions {
    fn new(_options: &str) -> Result<Self> {
        Ok(Self {})
    }
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {}
    }
}

pub fn player_from_id(player: player_id) -> Color {
    Color::from_u8(player - 1).expect("invalid player id")
}

pub fn player_to_id(player: Color) -> player_id {
    return u8::from(player) + 1;
}

impl GameMethods for ThreePlayerChessGame {
    fn create(init_info: &GameInit) -> Result<(Self, buf_sizer)> {
        let (options, state) = match *init_info {
            GameInit::Default => (None, None),
            GameInit::Standard {
                opts,
                legacy,
                state,
            } => {
                if legacy.is_some() {
                    return Err(Error::new_static(
                        ErrorCode::InvalidLegacy,
                        "legacys are not supported\0",
                    ));
                }
                (opts, state)
            }
            GameInit::Serialized(_) => {
                return Err(Error::new_static(
                    ErrorCode::FeatureUnsupported,
                    "serialized init info is not supported\0",
                ))
            }
        };

        let options = options
            .map(GameOptions::new)
            .transpose()?
            .unwrap_or_default();
        let game = if let Some(state_str) = state {
            ThreePlayerChess::from_str(state_str).map_err(|err_str| {
                // new static doesnt work unforunately because we don't have a zero terminator
                Error::new_dynamic(ErrorCode::InvalidState, err_str.to_owned())
            })?
        } else {
            ThreePlayerChess::default()
        };
        let game = Self {
            options,
            board: game,
        };
        Ok((game, BUF_SIZER))
    }

    fn export_options(&mut self, str_buf: &mut StrBuf) -> Result<()> {
        // TODO(cmrs)
        write!(str_buf, "",).expect("writing options buffer failed");
        Ok(())
    }

    fn copy_from(&mut self, other: &mut Self) -> Result<()> {
        debug_assert_eq!(self.options, other.options, "options mismatch in copy_from");
        self.board = other.board.clone();

        Ok(())
    }

    fn import_state(&mut self, state_str: Option<&str>) -> Result<()> {
        self.board = if let Some(state_str) = state_str {
            ThreePlayerChess::from_str(state_str)
                .map_err(|err_str| Error::new_static(ErrorCode::InvalidState, err_str))?
        } else {
            ThreePlayerChess::default()
        };
        Ok(())
    }

    fn export_state(&mut self, str_buf: &mut StrBuf) -> Result<()> {
        str_buf
            .write_str(&self.board.state_string())
            .expect("writing state buffer failed");

        Ok(())
    }

    fn players_to_move(&mut self, players: &mut PtrVec<player_id>) -> Result<()> {
        if self.board.game_status == GameStatus::Ongoing {
            players.push(player_to_id(self.board.turn));
        }
        Ok(())
    }

    fn get_concrete_moves(
        &mut self,
        player: player_id,
        moves: &mut PtrVec<move_code>,
    ) -> Result<()> {
        let player = player_from_id(player);
        if player == self.board.turn {
            for mov in self.board.gen_moves() {
                moves.push(mov.into());
            }
        }
        Ok(())
    }

    fn get_move_code(&mut self, _player: player_id, string: &str) -> Result<move_code> {
        Move::from_str(&mut self.board, string)
            .map(|mov| mov.into())
            .ok_or_else(|| {
                Error::new_dynamic(InvalidInput, format!("failed to parse move '{string}'"))
            })
    }

    fn get_move_str(
        &mut self,
        _player: player_id,
        mov: move_code,
        str_buf: &mut StrBuf,
    ) -> Result<()> {
        write!(
            str_buf,
            "{}",
            Move::try_from(mov)
                .expect("invalid move code")
                .to_string(&mut self.board)
        )
        .expect("writing move buffer failed");
        Ok(())
    }

    fn make_move(&mut self, player: player_id, mov: move_code) -> Result<()> {
        assert!(
            player_from_id(player) == self.board.turn,
            "attempted to make a move for a player whose turn it is currently not"
        );
        let tpc_move = Move::try_from(mov).expect("failed to parse move code");
        assert!(
            self.board.is_valid_move(tpc_move),
            "attempted to make an illegal move"
        );
        self.board.perform_move(tpc_move);
        Ok(())
    }

    fn get_results(&mut self, players: &mut PtrVec<player_id>) -> Result<()> {
        match self.board.game_status {
            GameStatus::Ongoing => Ok(()),
            GameStatus::Win(player, _reason) => {
                players.push(player_to_id(player));
                Ok(())
            }
            GameStatus::Draw(_reason) => {
                // TODO (cmrs): what are we supposed to do here??
                // push all players?
                Ok(())
            }
        }
    }

    fn is_legal_move(&mut self, player: player_id, mov: move_code) -> Result<()> {
        if self.board.turn != player_from_id(player) {
            return Err(Error::new_static(
                InvalidInput,
                "it is not currently this player's turn\0",
            ));
        }
        let tpc_move = Move::try_from(mov)
            .map_err(|_| Error::new_static(InvalidInput, "failed to parse move code\0"))?;
        if !self.board.is_valid_move(tpc_move) {
            return Err(Error::new_static(InvalidInput, "invalid move\0"));
        }
        Ok(())
    }

    fn print(&mut self, str_buf: &mut StrBuf) -> Result<()> {
        str_buf
            .write_str(&self.board.to_string())
            .expect("writing print buffer failed");
        Ok(())
    }
}
