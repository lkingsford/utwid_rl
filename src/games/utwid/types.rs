use super::Dir;

pub(crate) type ActorId = usize; // If I keep using this code, this might need to be u64, or something else
pub(crate) const CARDINAL_DIRS: [(Dir, isize, isize); 4] = [
    (Dir::N, 0, -1),
    (Dir::S, 0, 1),
    (Dir::E, 1, 0),
    (Dir::W, -1, 0),
];
pub(crate) const DIAGONAL_DIRS: [(Dir, isize, isize); 4] = [
    (Dir::NE, 1, -1),
    (Dir::NW, -1, -1),
    (Dir::SE, 1, 1),
    (Dir::SW, -1, 1),
];
pub(crate) const ACTOR_TYPE_NAMES: [&str; 5] = ["you", "monte", "them", "are", "one"];
pub(crate) const ACTOR_TYPE_YOU: usize = 0;
pub(crate) const ACTOR_TYPE_THEM: usize = 2;
pub(crate) const ACTOR_TYPE_ARE: usize = 3;
pub(crate) const ACTOR_TYPE_ONE: usize = 4;
pub(crate) const MON2Y_ID: usize = 1;
pub(crate) const PLAYER_MAX_HEALTH: usize = 7;
pub(crate) const ROOM_SPLITS_MIN: usize = 2;
pub(crate) const ROOM_SPLITS_MAX: usize = 8;
pub(crate) const PRESCRIPTION_TURNS: usize = 5;
pub(crate) const VISUAL_ROOMS: usize = 7;
pub(crate) const CONTENTION_WIDTH: usize = 4;

#[derive(Clone, std::fmt::Debug, PartialEq)]
pub enum GameState {
    Ongoing,
    Won,
    Lost,
    Checkpoint,
    Mon2yShortcircuit,
}

pub(crate) const YOU_ID: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Repr {
    Floor,
    Wall,
    Stairs,
    Win,
    You,
    Monte,
    Them,
    Are,
    One,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReprSet {
    Room1,
    Room2,
    Room3,
    Room4,
    Room5,
    Room6,
    Room7,
}
