use bitflags::bitflags;
use rand::{prelude::*, rngs::SmallRng};

use super::types::*;
use super::*;

pub const LEVEL_COUNT: usize = 9;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct TileTraits: u8 {
        const WALKABLE = 1 << 0;
        const STAIRS = 1 << 1;
        const WIN = 1 << 2;
    }
}

#[derive(Clone)]
pub struct Tile {
    pub(crate) traits: TileTraits,
    pub(crate) health: Option<isize>,
    pub repr: Option<Repr>,
    pub repr_set: ReprSet,
}

impl Tile {
    fn floor() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE,
            repr: Some(Repr::Floor),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    pub(crate) fn wall() -> Tile {
        Tile {
            traits: TileTraits::empty(),
            repr: Some(Repr::Wall),
            repr_set: ReprSet::Room1,
            health: Some(5),
        }
    }

    fn stair() -> Tile {
        Tile {
            traits: TileTraits::STAIRS | TileTraits::WALKABLE,
            repr: Some(Repr::Stairs),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    fn win() -> Tile {
        Tile {
            traits: TileTraits::WALKABLE | TileTraits::WIN,
            repr: Some(Repr::Win),
            repr_set: ReprSet::Room1,
            health: None,
        }
    }

    pub fn repr(&self) -> Option<Repr> {
        self.repr
    }

    pub fn modify_health(&mut self, dhealth: isize) {
        if self.health.is_some() {
            self.health = Some(self.health.unwrap() + dhealth);

            if self.health.unwrap() <= 0 {
                self.traits = TileTraits::WALKABLE;
                self.repr = Some(Repr::Floor);
                self.health = None;
            }
        }
    }
}

#[derive(Clone)]
pub struct Board {
    pub geography: Vec<Tile>,
    pub width: usize,
    pub height: usize,
    pub rng: SmallRng,
}

pub(crate) fn apply_dir(x: usize, y: usize, direction: Dir) -> (usize, usize) {
    let (_, dx, dy) = CARDINAL_DIRS
        .iter()
        .chain(DIAGONAL_DIRS.iter())
        .find(|(action, _, _)| action == &direction)
        .unwrap()
        .clone();

    // Perform arithmetic with isize to handle negative deltas correctly
    let new_x = x as isize + dx;
    let new_y = y as isize + dy;

    // These should always be non-negative due to prior filtering by board_permitted_moves
    (new_x as usize, new_y as usize)
}

fn room_to_repr_set(room: usize) -> ReprSet {
    match room % VISUAL_ROOMS {
        0 => ReprSet::Room1,
        1 => ReprSet::Room2,
        2 => ReprSet::Room3,
        3 => ReprSet::Room4,
        4 => ReprSet::Room5,
        5 => ReprSet::Room6,
        6 => ReprSet::Room7,
        _ => ReprSet::Room1,
    }
}

impl Board {
    pub fn new(_level: usize, rng: &mut SmallRng) -> Self {
        let (geography, width, height, rng) = Board::rooms_builder(_level, rng);
        Board {
            geography,
            width,
            height,
            rng,
        }
    }
    fn rooms_builder(_level: usize, rng: &mut SmallRng) -> (Vec<Tile>, usize, usize, SmallRng) {
        let mut rng = rng.clone();
        let width: usize = BOARD_WIDTH;
        let height: usize = BOARD_HEIGHT;
        let mut geography = vec![Tile::floor(); width * height];

        // Track vertical and horizontal positions separately to prevent parallel crowding
        let mut used_x: Vec<usize> = Vec::new();
        let mut used_y: Vec<usize> = Vec::new();
        let mut splits: Vec<(usize, usize)> = Vec::new();
        let mut split_repr_sets: Vec<((usize, usize), ReprSet)> = Vec::new();
        let mut room_number: usize = 1;

        'split_attempt: for _ in
            0..rng.random_range(ROOM_SPLITS_MIN + _level..ROOM_SPLITS_MAX + _level)
        {
            let vertical = rng.random_bool(0.5);
            let split_x = rng.random_range(1..width - 1);
            let split_y = rng.random_range(1..height - 1);
            let split_idx = split_x + split_y * width;
            let split_repr_set = geography[split_idx].repr_set;

            if vertical {
                if used_x.iter().any(|&x| split_x.abs_diff(x) <= 3) {
                    continue 'split_attempt;
                }

                if !geography[split_x + split_y * width]
                    .traits
                    .contains(TileTraits::WALKABLE)
                {
                    continue 'split_attempt;
                }

                for y in (0..=split_y).rev() {
                    let idx = split_x + y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                for y in (split_y + 1)..height {
                    let idx = split_x + y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }

                for x in (split_x + 1)..width {
                    let idx = x + split_y * width;
                    if !geography[idx].traits.contains(TileTraits::WALKABLE) {
                        break;
                    }
                    for y in (0..=split_y).rev() {
                        let idx = x + y * width;
                        if geography[idx].traits.contains(TileTraits::WALKABLE)
                            && !splits.contains(&(x, y))
                        {
                            geography[idx].repr_set = room_to_repr_set(room_number);
                        } else {
                            break;
                        }
                    }
                    for y in (split_y + 1)..height {
                        let idx = x + y * width;
                        if geography[idx].traits.contains(TileTraits::WALKABLE)
                            && !splits.contains(&(x, y))
                        {
                            geography[idx].repr_set = room_to_repr_set(room_number);
                        } else {
                            break;
                        }
                    }
                }

                used_x.push(split_x);
            } else {
                if used_y.iter().any(|&y| split_y.abs_diff(y) <= 3) {
                    continue 'split_attempt;
                }

                if !geography[split_x + split_y * width]
                    .traits
                    .contains(TileTraits::WALKABLE)
                {
                    continue 'split_attempt;
                }

                for x in (0..=split_x).rev() {
                    let idx = x + split_y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }
                for x in (split_x + 1)..width {
                    let idx = x + split_y * width;
                    if geography[idx].traits.contains(TileTraits::WALKABLE) {
                        geography[idx] = Tile::wall();
                    } else {
                        break;
                    }
                }

                for y in (split_y + 1)..height {
                    let idx = split_x + y * width;
                    if !geography[idx].traits.contains(TileTraits::WALKABLE) {
                        break;
                    }
                    for x in (0..=split_x).rev() {
                        let idx = x + y * width;
                        if geography[idx].traits.contains(TileTraits::WALKABLE)
                            && !splits.contains(&(x, y))
                        {
                            geography[idx].repr_set = room_to_repr_set(room_number);
                        } else {
                            break;
                        }
                    }
                    for x in (split_x + 1)..width {
                        let idx = x + y * width;
                        if geography[idx].traits.contains(TileTraits::WALKABLE)
                            && !splits.contains(&(x, y))
                        {
                            geography[idx].repr_set = room_to_repr_set(room_number);
                        } else {
                            break;
                        }
                    }
                }
                used_y.push(split_y);
            }
            geography[split_idx] = Tile::floor();
            geography[split_idx].repr_set = split_repr_set;

            room_number += 1;

            splits.push((split_x, split_y));
            split_repr_sets.push(((split_x, split_y), split_repr_set));
        }

        // Ensure stairs don't spawn in a wall
        let mut stair_idx;
        loop {
            let x = rng.random_range(0..width);
            let y = rng.random_range(0..height);
            stair_idx = x + y * width;
            if geography[stair_idx].traits.contains(TileTraits::WALKABLE) {
                break;
            }
        }

        let stair_repr_set = geography[stair_idx].repr_set;
        geography[stair_idx] = if _level < LEVEL_COUNT {
            Tile::stair()
        } else {
            Tile::win()
        };
        geography[stair_idx].repr_set = stair_repr_set;

        for (coords, split_repr_set) in split_repr_sets {
            let split_idx = coords.0 + coords.1 * width;
            geography[split_idx] = Tile::floor();
            geography[split_idx].repr_set = split_repr_set;
        }

        (geography, width, height, rng)
    }

    pub(crate) fn get(&self, x: usize, y: usize) -> &Tile {
        &self.geography[self.width * y + x]
    }

    pub(crate) fn get_mut(&mut self, x: usize, y: usize) -> &mut Tile {
        &mut self.geography[self.width * y + x]
    }

    pub(crate) fn board_permitted_moves(
        &self,
        from_x: usize,
        from_y: usize,
        cardinal: bool,
        diagonal: bool,
        melee: bool,
    ) -> Vec<Dir> {
        CARDINAL_DIRS
            .iter()
            .filter(|_| cardinal)
            .chain(DIAGONAL_DIRS.iter().filter(|_| diagonal))
            .filter_map(|(action, dx, dy)| {
                let x = from_x as isize + *dx as isize;
                let y = from_y as isize + *dy as isize;

                if x >= 0 && (x as usize) < self.width && y >= 0 && (y as usize) < self.height {
                    let tile = self.get(x as usize, y as usize);
                    (tile.traits.contains(TileTraits::WALKABLE) || (melee && tile.health.is_some()))
                        .then_some(*action)
                } else {
                    None
                }
            })
            .collect()
    }
}
