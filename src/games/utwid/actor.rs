use bitflags::bitflags;

use super::types::*;

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ActorTraits: u16 {
        const HUMAN = 1 << 0;
        const MON2Y = 1 << 1;
        const CARDINAL_MOVE = 1 << 2;
        const DIAGONAL_MOVE = 1 << 3;
        const WAIT = 1 << 4;
        const DEAD = 1 << 5;
        const MELEE = 1 << 6;
        const BOMB = 1 << 7;
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum Allegiance {
    You,
    Monty,
}

#[derive(Clone)]
pub struct Mon2yData {
    pub tree_id: u8,
    pub iterations: usize,
}

#[derive(Clone)]
pub struct GameActor {
    pub x: usize,
    pub y: usize,
    pub actor_type: usize,
    pub traits: ActorTraits,
    pub mon2y: Option<Mon2yData>,
    pub repr: Option<Repr>,
    pub repr_set: ReprSet,
    pub health: Option<usize>,
    pub attack_damage: Option<usize>,
    pub allegiance: Allegiance,
}

impl GameActor {
    pub fn repr(&self) -> Option<Repr> {
        self.repr
    }

    pub fn actor_type_name(&self) -> &'static str {
        ACTOR_TYPE_NAMES
            .get(self.actor_type)
            .copied()
            .unwrap_or("unknown")
    }

    pub fn modify_health(&mut self, d_health: isize) -> () {
        let current_health = self.health.unwrap_or(0);
        let new_health = (current_health as isize + d_health).max(0) as usize;
        self.health = Some(new_health);

        if new_health <= 0 {
            self.traits.insert(ActorTraits::DEAD);
        }
    }
}

impl GameActor {
    // Feels logical that these should be seperate
    pub(crate) fn you_actor() -> GameActor {
        GameActor {
            x: 1,
            y: 3,
            actor_type: ACTOR_TYPE_YOU,
            traits: ActorTraits::HUMAN
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::MELEE,
            mon2y: None,
            repr: Some(Repr::You),
            repr_set: ReprSet::Room1,
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::You,
        }
    }

    pub(crate) fn monte_actor() -> GameActor {
        GameActor {
            x: 7,
            y: 7,
            actor_type: 1,
            traits: ActorTraits::MON2Y
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::WAIT
                | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Monte),
            repr_set: ReprSet::Room1,
            health: Some(7),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    pub(crate) fn them_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_THEM,
            traits: ActorTraits::MON2Y | ActorTraits::DIAGONAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Them),
            repr_set: ReprSet::Room1,
            health: Some(2),
            attack_damage: Some(1),
            allegiance: Allegiance::Monty,
        }
    }

    pub(crate) fn are_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_ARE,
            traits: ActorTraits::MON2Y | ActorTraits::CARDINAL_MOVE | ActorTraits::MELEE,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 1000,
            }),
            repr: Some(Repr::Are),
            repr_set: ReprSet::Room1,
            health: Some(3),
            attack_damage: Some(2),
            allegiance: Allegiance::Monty,
        }
    }

    pub(crate) fn one_actor(x: usize, y: usize) -> GameActor {
        GameActor {
            x,
            y,
            actor_type: ACTOR_TYPE_ONE,
            traits: ActorTraits::MON2Y
                | ActorTraits::DIAGONAL_MOVE
                | ActorTraits::CARDINAL_MOVE
                | ActorTraits::BOMB,
            mon2y: Some(Mon2yData {
                tree_id: 1,
                iterations: 500,
            }),
            repr: Some(Repr::One),
            repr_set: ReprSet::Room1,
            health: Some(4),
            attack_damage: Some(5),
            allegiance: Allegiance::Monty,
        }
    }
}
