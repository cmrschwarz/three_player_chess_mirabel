//! _mirabel_ frontend plugin for _Connect Four_.

use crate::game::*;
use mirabel::{
    cstr,
    frontend::{
        create_frontend_methods, frontend_feature_flags, frontend_methods, FrontendMethods,
        Metadata,
    },
    imgui, plugin_get_frontend_methods, semver,
    sys::SDL_BUTTON_LEFT,
    sys::SDL_BUTTON_RIGHT,
    ErrorCode, EventAny, ValidCStr,
};
use nalgebra::Vector2;
use surena_game::Error;
use surena_game::GameMethods;
use three_player_chess::board::{Move, ThreePlayerChess};
use three_player_chess_frontend::*;
pub const FRONTEND_NAME: &str = "three_player_chess_frontend\0";

struct MirabelFrontend {
    fe: three_player_chess_frontend::Frontend,
    disabled: bool,
}

impl std::ops::Deref for MirabelFrontend {
    type Target = Frontend;

    fn deref(&self) -> &Self::Target {
        return &self.fe;
    }
}
impl std::ops::DerefMut for MirabelFrontend {
    fn deref_mut(&mut self) -> &mut Self::Target {
        return &mut self.fe;
    }
}

impl FrontendMethods for MirabelFrontend {
    type Options = ();

    fn create(_options: Option<&Self::Options>) -> mirabel::Result<Self> {
        Ok(Self {
            fe: Frontend::new(),
            disabled: true,
        })
    }

    fn runtime_opts_display(fe: mirabel::frontend::Wrapped<Self>) -> mirabel::Result<()> {
        // No runtime options.
        imgui::check_box(
            ValidCStr::try_from("Transformed Pieces\0").unwrap(),
            &mut fe.frontend.transformed_pieces,
        );
        imgui::check_box(
            ValidCStr::try_from("Transform Dragged Piece\0").unwrap(),
            &mut fe.frontend.transform_dragged_pieces,
        );
        imgui::check_box(
            ValidCStr::try_from("Highlight Attacked Pieces\0").unwrap(),
            &mut fe.frontend.highlight_attacked,
        );
        Ok(())
    }

    fn process_event(
        mut fe: mirabel::frontend::Wrapped<Self>,
        event: mirabel::EventAny,
    ) -> mirabel::Result<()> {
        match event.to_rust() {
            mirabel::EventEnum::GameLoadMethods(e) => {
                // TODO(cmrs): this is kinda hacky, find a better way to do this
                // especially once we have options
                let tpcg = ThreePlayerChessGame::create(&e.init_info)?.0;
                fe.reset();
                fe.board = tpcg.board;
                fe.disabled = false;
            }
            mirabel::EventEnum::GameUnload(_) => {
                fe.disabled = true;
            }
            mirabel::EventEnum::GameState(e) => {
                if let Some(state_str) = e.state {
                    fe.board = ThreePlayerChess::from_str(state_str.into()).map_err(|err_str| {
                        Error::new_dynamic(ErrorCode::InvalidState, err_str.to_owned())
                    })?;
                    fe.disabled = false;
                }
            }
            mirabel::EventEnum::GameMove(e) => {
                let tpc_mov = Move::try_from(e.code).map_err(|_| {
                    Error::new_static(ErrorCode::InvalidMove, "invalid move code\0")
                })?;
                fe.board.perform_move(tpc_mov);
                fe.reset_effects();
            }
            _ => (),
        }

        Ok(())
    }

    fn process_input(
        mut frontend: mirabel::frontend::Wrapped<Self>,
        event: mirabel::SDLEventEnum,
    ) -> mirabel::Result<()> {
        if frontend.disabled {
            return Ok(());
        }
        // TODO (cmrs): this is a really hacky way to figure out whether
        // moves were made on the frontend side.
        // we could do a lot better here e.g. if mouse_clicked returned
        // moves that were made. but for now ... good enough
        let history_len = frontend.history.len();
        let mut current_turn = frontend.board.turn;

        match event {
            mirabel::SDLEventEnum::MouseMotion(e) => {
                frontend.mouse_moved(Vector2::new(e.x, e.y));
            }
            mirabel::SDLEventEnum::MouseButtonDown(e) => {
                frontend.mouse_moved(Vector2::new(e.x, e.y));
                let btn = u32::from(e.button);
                if btn == SDL_BUTTON_LEFT {
                    frontend.mouse_clicked(false);
                } else if btn == SDL_BUTTON_RIGHT {
                    frontend.mouse_clicked(true);
                }
            }
            mirabel::SDLEventEnum::MouseButtonUp(e) => {
                frontend.mouse_moved(Vector2::new(e.x, e.y));
                let btn = u32::from(e.button);
                if btn == SDL_BUTTON_LEFT {
                    frontend.mouse_released(false);
                } else if btn == SDL_BUTTON_RIGHT {
                    frontend.mouse_released(true);
                }
            }
            _ => (),
        };

        // since we can't borrow the outbox and the history at the same
        // we can't just use an iterator here
        let mut history_idx = history_len;
        loop {
            if let Some((rm, _)) = frontend.history.get(history_idx) {
                frontend.outbox.push(&mut EventAny::new_game_move(
                    player_to_id(current_turn),
                    rm.mov.into(),
                ));
                history_idx += 1;
                current_turn = current_turn.next();
            } else {
                break;
            }
        }
        // mirabel wants to make the moves itself again
        for _ in history_len..history_idx {
            frontend.undo_move();
        }

        Ok(())
    }

    fn update(mut _frontend: mirabel::frontend::Wrapped<Self>) -> mirabel::Result<()> {
        Ok(())
    }

    fn render(mut frontend: mirabel::frontend::Wrapped<Self>) -> mirabel::Result<()> {
        let canvas = frontend.canvas.get();
        let tpc_fe = &mut frontend.frontend.fe;
        let dd = frontend.display_data;

        if frontend.frontend.disabled {
            canvas.clear(tpc_fe.background);
            return Ok(());
        }

        // TODO(cmrs): this code should really live in the frontend, not here.
        // but because of mirabels viewport shenanigans we have to do this
        // because the frontend render() method will not respect these offsets
        tpc_fe.board_origin = Vector2::new((dd.x + dd.w / 2.) as i32, (dd.y + dd.h / 2.) as i32);
        tpc_fe.board_radius = std::cmp::min(dd.w as i32, dd.h as i32) as f32 * 0.46;
        tpc_fe.hovered_square = None;
        if tpc_fe.dragged_square.is_some() {
            if let Some(sq) = tpc_fe.get_board_pos_from_screen_pos(tpc_fe.cursor_pos) {
                if tpc_fe.possible_moves[usize::from(sq)] {
                    tpc_fe.hovered_square = Some(sq);
                }
            }
        }

        let translation = tpc_fe.board_origin.cast::<f32>();

        canvas.save();
        canvas.translate((translation.x - dd.x, translation.y - dd.y));
        canvas.scale((tpc_fe.board_radius, tpc_fe.board_radius));
        canvas.save();

        tpc_fe.render_background(canvas);
        canvas.restore();

        canvas.save();
        tpc_fe.render_notation(canvas);
        canvas.restore();

        for c in three_player_chess::board::Color::iter() {
            for right in [true, false] {
                canvas.save();
                tpc_fe.render_hexboard(canvas, *c, right);
                canvas.restore();
            }
        }

        canvas.save();
        tpc_fe.render_dragged_piece(canvas);
        canvas.restore();

        canvas.restore();
        Ok(())
    }

    fn is_game_compatible(game: mirabel::frontend::GameInfo) -> mirabel::CodeResult<()> {
        if game.game_name == strip_nul(GAME_NAME)
            && game.impl_name == strip_nul(IMPL_NAME)
            && game.variant_name == strip_nul(VARIANT_NAME)
        {
            Ok(())
        } else {
            Err(ErrorCode::FeatureUnsupported)
        }
    }
}

/// Generate [`frontend_methods`] struct.
fn three_player_chess_frontend() -> frontend_methods {
    create_frontend_methods::<MirabelFrontend>(Metadata {
        frontend_name: cstr(FRONTEND_NAME),
        version: semver {
            major: 0,
            minor: 1,
            patch: 0,
        },
        features: frontend_feature_flags::default(),
    })
}

plugin_get_frontend_methods!(three_player_chess_frontend());

/// Strip NUL character from `s`.
///
/// # Panics
/// Panics if `s` is not NUL-terminated.
fn strip_nul(s: &str) -> &str {
    s.strip_suffix('\0')
        .expect("string slice not NUL-terminated")
}
