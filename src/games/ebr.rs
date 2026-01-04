use crate::hyper::{GameHyperrewardTrait, Hyperparams};
use log::warn;
use serde::{de, Deserializer, Serialize, Serializer};
use serde_json;
use std::cmp::max;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, LazyLock, OnceLock};

use crate::game::Game;
use crate::mcts::game_trait::{Action, Actor, State};

// ANNOTATION: This file is a machine-readable implementation of the board game rules
// found in `Rules.md`. It is primarily intended for playtesting and simulation via
// an MCTS agent. As such, some rules may be simplified, hardcoded for a specific
// scenario, or marked with TODO/FIXME where the implementation diverges from the
// official rules document. Key differences will be noted in comments.

/*
OK - here's the deal. This is to help me playtest something.
It's a lot quicker for me to shove the data directly in the
source file, though I know it would be better for it to be in
data files. It's serving its purpose, and it doesn't need to
be built for maintainability.
*/

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Default)]
pub enum EndGameReason {
    #[default]
    InProgress,
    Shares,
    Bonds,
    Track,
    Resources,
    Stalemate,
    Dividends,
    Bankruptcy,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct EBRHyperrewards {
    pub total_bonds_issued: usize,
    pub end_game_reason: EndGameReason,
    pub remaining_resource_cubes: usize,
    pub ebrc_connected_to_devonport: bool,
    pub ebrc_connected_to_launceston: bool,
    pub ebrc_connected_to_hobart: bool,
    pub lw_connected_to_devonport: bool,
    pub lw_connected_to_launceston: bool,
    pub lw_connected_to_hobart: bool,
    pub tmlc_connected_to_devonport: bool,
    pub tmlc_connected_to_launceston: bool,
    pub tmlc_connected_to_hobart: bool,
    pub gt_connected_to_devonport: bool,
    pub gt_connected_to_launceston: bool,
    pub gt_connected_to_hobart: bool,
    pub nmft_connected_to_devonport: bool,
    pub nmft_connected_to_launceston: bool,
    pub nmft_connected_to_hobart: bool,
    pub ned_connected_to_devonport: bool,
    pub ned_connected_to_launceston: bool,
    pub ned_connected_to_hobart: bool,
    pub mlm_connected_to_devonport: bool,
    pub mlm_connected_to_launceston: bool,
    pub mlm_connected_to_hobart: bool,
    pub completed_dividend_rounds: usize,
    pub gt_merged: bool,
    pub nmft_merged: bool,
    pub ned_merged: bool,
    pub mlm_merged: bool,
    pub lw_auction_winner: Option<usize>,
    pub tmlc_auction_winner: Option<usize>,
    pub ebrc_auction_winner: Option<usize>,
    pub gt_auction_winner: Option<usize>,
    pub winning_player_id: Option<usize>,
    pub winning_player_score: Option<isize>,
    pub player_scores: Vec<isize>,
    pub overall_track_ratio: f32,
    pub terrain_track_ratios: HashMap<Terrain, f32>,
}

impl GameHyperrewardTrait for EBRHyperrewards {
    fn meta() -> HashMap<String, String> {
        let mut meta = HashMap::new();
        let default_rewards = EBRHyperrewards::default();
        let json_value = serde_json::to_value(default_rewards).unwrap();
        let json_object = json_value.as_object().unwrap();

        for (key, value) in json_object {
            let type_str = match value {
                serde_json::Value::Bool(_) => "bool".to_string(),
                serde_json::Value::Number(_) => "int".to_string(),
                // Enums are serialized as strings.
                serde_json::Value::String(_) => "string".to_string(),
                _ => "unknown".to_string(),
            };
            meta.insert(key.clone(), type_str);
        }
        meta
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct WaterFeatureParams {
    pub xy: (u8, u8),
    pub feature_type: FeatureType,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct TerrainAttributeParams {
    pub build_cost: u32,
    pub revenue: [isize; 6],
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct CompanyFixedDetailsParams {
    pub starting: Option<Coordinate>,
    pub private: bool,
    pub stock_available: usize,
    pub track_available: usize,
    pub initial_treasury: usize,
    pub initial_interest: usize,
}

mod tuple_map_as_vec {
    use serde::{de::Deserializer, ser::Serializer, Deserialize, Serialize};
    use std::collections::HashMap;
    use std::hash::Hash;

    pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize + Eq + Hash,
        V: Serialize,
    {
        let vec: Vec<_> = map.iter().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
    {
        let vec: Vec<(K, V)> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct EBRHyperparams {
    pub terrain_attributes: HashMap<Terrain, TerrainAttributeParams>,
    #[serde(with = "tuple_map_as_vec")]
    pub features: HashMap<(usize, usize), Feature>,
    #[serde(with = "tuple_map_as_vec")]
    pub water_features: HashMap<(usize, usize), FeatureType>,
    pub bonds: Vec<Bond>,
    pub initial_cash: HashMap<u8, u32>,
    pub company_fixed_details: HashMap<Company, CompanyFixedDetailsParams>,
    pub water_1_cost: usize,
    pub water_2_cost: usize,
    pub narrow_gauge_initial: usize,
    pub max_builds: usize,
    pub narrow_track_cost: usize,
    pub take_resource_cost: u32,
    pub take_dividend: u32,
    pub take_town_deliver_dividend: u32,
    pub take_port_deliver_dividend: u32,
    pub initial_resource_cubes: Vec<Coordinate>,
    #[serde(skip)]
    all_features_cache: Arc<OnceLock<Arc<HashMap<(usize, usize), Feature>>>>,
}

impl Hyperparams for EBRHyperparams {}

impl EBRHyperparams {
    fn default_cached() -> &'static EBRHyperparams {
        static ONCE: OnceLock<EBRHyperparams> = OnceLock::new();
        ONCE.get_or_init(EBRHyperparams::default)
    }

    fn all_features(&self) -> Arc<HashMap<(usize, usize), Feature>> {
        self.all_features_cache
            .get_or_init(|| {
                Arc::new(
                    self.features
                        .clone()
                        .into_iter()
                        .chain(self.water_features.clone().into_iter().map(
                            |((x, y), feature_type)| {
                                let cost = match feature_type {
                                    FeatureType::Water1 => self.water_1_cost,
                                    FeatureType::Water2 => self.water_2_cost,
                                    _ => unreachable!(),
                                };
                                (
                                    (x, y),
                                    Feature {
                                        feature_type,
                                        location_name: None,
                                        revenue: [0, 0, 0, 0, 0, 0],
                                        additional_cost: cost,
                                    },
                                )
                            },
                        ))
                        .collect(),
                )
            })
            .clone()
    }

    fn default_terrain_attributes() -> HashMap<Terrain, TerrainAttributeParams> {
        let mut terrain_attributes = HashMap::new();
        terrain_attributes.insert(
            Terrain::Nothing,
            TerrainAttributeParams {
                build_cost: 0,
                revenue: [0, 0, 0, 0, 0, 0],
            },
        );
        terrain_attributes.insert(
            Terrain::Plain,
            TerrainAttributeParams {
                build_cost: 3,
                revenue: [0, 0, 0, 0, 0, 0],
            },
        );
        terrain_attributes.insert(
            Terrain::Forest,
            TerrainAttributeParams {
                build_cost: 4,
                revenue: [1, 1, 1, 1, 0, 0],
            },
        );
        terrain_attributes.insert(
            Terrain::Mountain,
            TerrainAttributeParams {
                build_cost: 6,
                revenue: [0, 0, 0, 0, 0, 0],
            },
        );
        terrain_attributes.insert(
            Terrain::Town,
            TerrainAttributeParams {
                build_cost: 4,
                revenue: [0, 0, 0, 0, 0, 0],
            },
        );
        terrain_attributes.insert(
            Terrain::Port,
            TerrainAttributeParams {
                build_cost: 5,
                revenue: [0, 0, 0, 0, 0, 0],
            },
        );
        terrain_attributes
    }

    fn default_initial_cash() -> HashMap<u8, u32> {
        let mut initial_cash = HashMap::new();
        initial_cash.insert(2, 20);
        initial_cash.insert(3, 13);
        initial_cash.insert(4, 10);
        initial_cash.insert(5, 8);
        initial_cash
    }

    fn default_bonds() -> Vec<Bond> {
        vec![
            Bond {
                face_value: 5,
                coupon: 1,
            },
            Bond {
                face_value: 5,
                coupon: 1,
            },
            Bond {
                face_value: 10,
                coupon: 3,
            },
            Bond {
                face_value: 10,
                coupon: 3,
            },
            Bond {
                face_value: 10,
                coupon: 4,
            },
            Bond {
                face_value: 15,
                coupon: 4,
            },
            Bond {
                face_value: 15,
                coupon: 5,
            },
        ]
    }

    fn default_company_fixed_details() -> HashMap<Company, CompanyFixedDetailsParams> {
        let mut company_fixed_details = HashMap::new();
        company_fixed_details.insert(
            Company::EBRC,
            CompanyFixedDetailsParams {
                starting: Some((3, 5)),
                private: false,
                stock_available: 5,
                track_available: 10,
                initial_treasury: 0,
                initial_interest: 0,
            },
        );
        company_fixed_details.insert(
            Company::LW,
            CompanyFixedDetailsParams {
                starting: Some((9, 4)),
                private: false,
                stock_available: 3,
                track_available: 10,
                initial_treasury: 0,
                initial_interest: 0,
            },
        );
        company_fixed_details.insert(
            Company::TMLC,
            CompanyFixedDetailsParams {
                starting: Some((9, 4)),
                private: false,
                stock_available: 4,
                track_available: 10,
                initial_treasury: 0,
                initial_interest: 0,
            },
        );
        company_fixed_details.insert(
            Company::GT,
            CompanyFixedDetailsParams {
                starting: Some((2, 4)),
                private: true,
                stock_available: 1,
                track_available: 0,
                initial_treasury: 10,
                initial_interest: 2,
            },
        );
        company_fixed_details.insert(
            Company::NMFT,
            CompanyFixedDetailsParams {
                starting: None,
                private: true,
                stock_available: 1,
                track_available: 0,
                initial_treasury: 0,
                initial_interest: 0,
            },
        );
        company_fixed_details.insert(
            Company::NED,
            CompanyFixedDetailsParams {
                starting: None,
                private: true,
                stock_available: 1,
                track_available: 0,
                initial_treasury: 15,
                initial_interest: 3,
            },
        );
        company_fixed_details.insert(
            Company::MLM,
            CompanyFixedDetailsParams {
                starting: None,
                private: true,
                stock_available: 1,
                track_available: 0,
                initial_treasury: 20,
                initial_interest: 5,
            },
        );
        company_fixed_details
    }

    fn default_features() -> HashMap<(usize, usize), Feature> {
        let mut m = HashMap::new();
        m.insert(
            (2, 5),
            Feature {
                feature_type: FeatureType::Port,
                location_name: Some("Port of Strahan".to_string()),
                revenue: ([2, 2, 0, 0, 0, 0]),
                additional_cost: 0,
            },
        );
        m.insert(
            (10, 9),
            Feature {
                feature_type: FeatureType::Port,
                location_name: Some("Hobart".to_string()),
                revenue: ([5, 5, 4, 4, 3, 3]),
                additional_cost: 0,
            },
        );
        m.insert(
            (9, 9),
            Feature {
                feature_type: FeatureType::Town,
                location_name: Some("New Norfolk".to_string()),
                revenue: ([2, 2, 2, 2, 2, 2]),
                additional_cost: 0,
            },
        );
        m.insert(
            (2, 5),
            Feature {
                feature_type: FeatureType::Port,
                location_name: Some("Burnie".to_string()),
                revenue: ([2, 2, 1, 1, 0, 0]),
                additional_cost: 0,
            },
        );
        m.insert(
            (2, 6),
            Feature {
                feature_type: FeatureType::Town,
                location_name: Some("Ulverstone".to_string()),
                revenue: ([2, 2, 1, 1, 1, 1]),
                additional_cost: 0,
            },
        );
        m.insert(
            (7, 3),
            Feature {
                feature_type: FeatureType::Port,
                location_name: Some("Devonport".to_string()),
                revenue: ([3, 3, 1, 1, 0, 0]),
                additional_cost: 0,
            },
        );
        m.insert(
            (9, 4),
            Feature {
                feature_type: FeatureType::Port,
                location_name: Some("Launceston".to_string()),
                revenue: ([3, 3, 1, 1, 0, 0]),
                additional_cost: 0,
            },
        );
        m.insert(
            (3, 5),
            Feature {
                feature_type: FeatureType::Town,
                location_name: Some("Queenstown".to_string()),
                revenue: ([2, 2, 2, 2, 2, 2]),
                additional_cost: 0,
            },
        );

        m
    }

    fn default_water() -> HashMap<(usize, usize), FeatureType> {
        vec![
            (FeatureType::Water1, (8, 2)),
            (FeatureType::Water1, (8, 3)),
            (FeatureType::Water2, (8, 5)),
            (FeatureType::Water1, (9, 6)),
            (FeatureType::Water2, (3, 7)),
            (FeatureType::Water1, (4, 7)),
            (FeatureType::Water1, (6, 8)),
            (FeatureType::Water1, (6, 9)),
            (FeatureType::Water1, (10, 9)),
            (FeatureType::Water2, (5, 11)),
            (FeatureType::Water2, (9, 11)),
            (FeatureType::Water1, (6, 11)),
        ]
        .into_iter()
        .map(|(ft, (x, y))| ((x as usize, y as usize), ft))
        .collect()
    }
}

impl Default for EBRHyperparams {
    fn default() -> Self {
        EBRHyperparams {
            terrain_attributes: EBRHyperparams::default_terrain_attributes(),
            features: EBRHyperparams::default_features(),
            water_features: EBRHyperparams::default_water(),
            bonds: EBRHyperparams::default_bonds(),
            initial_cash: EBRHyperparams::default_initial_cash(),
            company_fixed_details: EBRHyperparams::default_company_fixed_details(),
            water_1_cost: 1,
            water_2_cost: 3,
            narrow_gauge_initial: 12,
            max_builds: 3,
            narrow_track_cost: 2,
            take_resource_cost: 3,
            take_dividend: 1,
            take_town_deliver_dividend: 1,
            take_port_deliver_dividend: 1,
            initial_resource_cubes: vec![(2, 4), (2, 3), (3, 4), (3, 4)],
            all_features_cache: Arc::new(OnceLock::new()),
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChoosableAction {
    BuildTrack,
    AuctionShare,
    TakeResources,
    IssueBond,
    Merge,
    PayDividend,
}

const ACTION_CUBE_SPACES: [ChoosableAction; 11] = [
    ChoosableAction::BuildTrack,
    ChoosableAction::BuildTrack,
    ChoosableAction::BuildTrack,
    ChoosableAction::AuctionShare,
    ChoosableAction::AuctionShare,
    ChoosableAction::TakeResources,
    ChoosableAction::TakeResources,
    ChoosableAction::TakeResources,
    ChoosableAction::IssueBond,
    ChoosableAction::Merge,
    ChoosableAction::PayDividend,
];

type ActionCubeSpaces = [bool; 11];

const ACTION_CUBE_INIT: ActionCubeSpaces = [
    // This might not be the most helpful way to mentally consider this
    // ANNOTATION: Represents the initial state of the action table.
    // `true` means a cube is present. This is a hardcoded setup for testing.
    // As defined, initial available actions are BuildTrack, AuctionShare, IssueBond, Merge.
    // Initially occupied actions are TakeResources (3 cubes) and PayDividend (1 cube).
    false, false, false, false, false, true, true, true, false, false, true,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub struct Bond {
    face_value: usize,
    coupon: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq)]
struct BondDetails {
    bond: Bond,
    deferred: bool,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Feature {
    feature_type: FeatureType,
    location_name: Option<String>,
    revenue: [isize; 6],
    additional_cost: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
pub enum FeatureType {
    Port,
    Town,
    Water1,
    Water2,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Copy)]
pub enum Company {
    EBRC,
    LW,
    TMLC,
    GT,
    NMFT,
    NED,
    MLM,
}

impl Serialize for Company {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match *self {
            Company::EBRC => "EBRC",
            Company::LW => "LW",
            Company::TMLC => "TMLC",
            Company::GT => "GT",
            Company::NMFT => "NMFT",
            Company::NED => "NED",
            Company::MLM => "MLM",
        })
    }
}

impl<'de> de::Deserialize<'de> for Company {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "EBRC" => Ok(Company::EBRC),
            "LW" => Ok(Company::LW),
            "TMLC" => Ok(Company::TMLC),
            "GT" => Ok(Company::GT),
            "NMFT" => Ok(Company::NMFT),
            "NED" => Ok(Company::NED),
            "MLM" => Ok(Company::MLM),
            _ => Err(de::Error::custom(format!("unknown company: {}", s))),
        }
    }
}

const ALL_COMPANIES: [Company; 7] = [
    Company::EBRC,
    Company::LW,
    Company::TMLC,
    Company::GT,
    Company::NMFT,
    Company::NED,
    Company::MLM,
];

const IPO_ORDER: [Company; 4] = [Company::LW, Company::TMLC, Company::EBRC, Company::GT];
static PRIVATE_ORDER: LazyLock<Vec<Company>> =
    LazyLock::new(|| vec![Company::GT, Company::NMFT, Company::NED, Company::MLM]);

type Coordinate = (usize, usize);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompanyDetails {
    shares_held: usize,
    shares_remaining: usize,
    merged: Option<bool>,
    cash: isize,
    available: Option<bool>,
    hq: Option<Coordinate>,
    track_remaining: usize,
    bonds: Vec<BondDetails>,
    owned_privates: Vec<Company>,
}

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
struct CommonAttributes {
    build_cost: u32,
    buildable: bool,
    multiple_allowed: bool,
    revenue: [isize; 6],
}

const FINAL_DIVIDEND_COUNT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Hash, Eq, PartialOrd, Ord)]
pub enum Terrain {
    Nothing,
    Plain,
    Forest,
    Mountain,
    Town,
    Port,
}

impl Serialize for Terrain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match *self {
            Terrain::Nothing => "nothing",
            Terrain::Plain => "plain",
            Terrain::Forest => "forest",
            Terrain::Mountain => "mountain",
            Terrain::Town => "town",
            Terrain::Port => "port",
        })
    }
}

impl<'de> de::Deserialize<'de> for Terrain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "nothing" => Ok(Terrain::Nothing),
            "plain" => Ok(Terrain::Plain),
            "forest" => Ok(Terrain::Forest),
            "mountain" => Ok(Terrain::Mountain),
            "town" => Ok(Terrain::Town),
            "port" => Ok(Terrain::Port),
            _ => Err(de::Error::custom(format!("unknown terrain: {}", s))),
        }
    }
}

const N: Terrain = Terrain::Nothing;
const P: Terrain = Terrain::Plain;
const F: Terrain = Terrain::Forest;
const M: Terrain = Terrain::Mountain;
const T: Terrain = Terrain::Town;
const R: Terrain = Terrain::Port;

const HEIGHT: usize = 13;
const WIDTH: usize = 14;

const TERRAIN: [[Terrain; WIDTH]; HEIGHT] = [
    /* */ [N, N, N, N, N, N, N, N, N, N, N, N, N, N],
    /*  */ [N, P, F, P, P, N, N, N, N, N, N, N, P, N],
    /* */ [N, F, F, F, P, R, T, N, P, N, F, F, F, M],
    /*   */ [N, F, F, M, P, P, P, R, P, P, P, F, F, F],
    /* */ [N, N, F, M, M, F, F, P, F, R, P, F, F, F],
    /*   */ [N, N, R, T, M, M, M, F, P, P, P, P, F, F],
    /* */ [N, N, N, F, M, M, M, F, P, P, P, P, F, F],
    /*   */ [N, N, N, M, M, M, M, F, P, P, P, P, P, N],
    /* */ [N, N, N, F, F, M, M, F, P, P, P, P, P, N],
    /*   */ [N, N, N, N, F, F, M, F, F, T, R, P, P, N],
    /* */ [N, N, N, N, N, F, M, F, F, F, N, N, N, N],
    /*   */ [N, N, N, N, N, F, F, F, F, F, N, N, N, N],
    /* */ [N, N, N, N, N, N, N, F, N, N, N, N, N, N],
];

static PRIVATE_STARTING_LOCATIONS: LazyLock<Vec<Coordinate>> = LazyLock::new(|| {
    TERRAIN
        .iter()
        .enumerate()
        .flat_map(|(y, column)| {
            column
                .iter()
                .enumerate()
                .filter(|(x, cell)| **cell == Terrain::Mountain || **cell == Terrain::Forest)
                .map(move |(x, _cell)| (x, y))
        })
        .collect::<Vec<Coordinate>>()
});
// Privates can start anywhere on a Forest or Mountain (without an existing HQ,
// but obviously, that bit is state dependent)
const INITIAL_TRACK: [Track; 4] = [
    Track {
        location: (9, 4),
        track_type: TrackType::CompanyOwned(Company::LW),
    },
    Track {
        location: (9, 4),
        track_type: TrackType::CompanyOwned(Company::TMLC),
    },
    Track {
        location: (3, 5),
        track_type: TrackType::CompanyOwned(Company::EBRC),
    },
    Track {
        location: (2, 4),
        track_type: TrackType::Narrow,
    },
];

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum EBRAction {
    Bid(usize),
    Pass,
    MoveCube(ChoosableAction, ChoosableAction),
    Stalemate,
    ChooseAuctionCompany(Company),
    StartPrivateAt(Company, Coordinate),
    ChooseBuildCompany(Company),
    BuildTrack(Coordinate),
    BuildPass,
    ChooseBondCompany(Company),
    IssueBond(Company, Bond),
    Merge(Company, Company),
    ChooseTakeResourcesCompany(Company, Option<Company>),
    TakeResources(Coordinate),
    PassTakeResources,
}

impl EBRAction {
    fn execute_stalemate(&self, state: &EBRState) -> EBRState {
        let mut state = state.clone();
        state.terminal = true;
        state.end_game_reason = EndGameReason::Stalemate;
        state
    }

    fn execute_bid(&self, state: &EBRState, bid: &usize) -> EBRState {
        let mut state = state.clone();
        let stage = state.stage;
        match stage {
            Stage::Auction {
                lot,
                initial_auction,
                passed,
                ..
            } => {
                let Actor::Player(actor) = state.next_actor else {
                    unreachable!()
                };
                let mut next_actor = (&actor + 1) % state.player_count;
                while passed.contains(&next_actor) {
                    next_actor = (&next_actor + 1) % state.player_count;
                }
                state.stage = Stage::Auction {
                    current_bid: Some(*bid as isize),
                    lot,
                    initial_auction,
                    winning_bidder: Some(actor),
                    passed,
                };
                state.next_actor = Actor::Player(next_actor);
            }
            _ => unreachable!(),
        }
        state
    }

    fn execute_pass(&self, state: &EBRState) -> EBRState {
        let mut state = state.clone();
        if let Stage::Auction {
            current_bid,
            lot,
            initial_auction,
            winning_bidder,
            mut passed,
        } = state.stage
        {
            // -2 because need all but one to have passed, and one
            // isn't on the list yet
            if passed.len() < (state.player_count - 2) as usize {
                let Actor::Player(mut next_actor) = state.next_actor else {
                    unreachable!()
                };
                passed.insert(next_actor as u8);
                while passed.contains(&next_actor) {
                    next_actor = (&next_actor + 1) % state.player_count;
                }
                state.next_actor = Actor::Player(winning_bidder.unwrap());
                state.stage = Stage::Auction {
                    initial_auction,
                    lot,
                    current_bid,
                    winning_bidder,
                    passed,
                };
                return state;
            };
            // Everybody has passed.
            let winner = winning_bidder.unwrap();
            state.holdings.get_mut(&winner).unwrap().push(lot);
            *state.player_cash.get_mut(&winner).unwrap() -= current_bid.unwrap_or(0) as isize;
            {
                let company_details = state.company_details.get_mut(&lot).unwrap();
                company_details.shares_held += 1;
                company_details.shares_remaining -= 1;
                company_details.cash += current_bid.unwrap();
            }
            if state.company_fixed_details[&lot].private {
                let index = PRIVATE_ORDER.iter().position(|c| *c == lot).unwrap();
                if index != PRIVATE_ORDER.len() - 1 {
                    state
                        .company_details
                        .get_mut(&PRIVATE_ORDER[index + 1])
                        .unwrap()
                        .available = Some(true);
                }
                state.company_details.get_mut(&lot).unwrap().available = Some(false);
            }
            // Either next player, or next auction (for initial auction)
            if initial_auction {
                state.initial_auction_winners.insert(lot, winner);
                if lot == Company::GT {
                    // End of initial auction
                    state.stage = Stage::ChooseAction;
                    state.next_actor = Actor::Player(winner);
                } else {
                    state.stage = Stage::Auction {
                        initial_auction: true,
                        current_bid: None,
                        // Todo: Use the constant
                        lot: match lot {
                            Company::LW => Company::TMLC,
                            Company::TMLC => Company::EBRC,
                            Company::EBRC => Company::GT,
                            _ => unreachable!(),
                        },
                        winning_bidder: None,
                        passed: HashSet::new(),
                    }
                }
            } else {
                state.stage = Stage::ChooseAction;
                state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
            }
        } else {
            unreachable!()
        }
        state
    }

    fn execute_move_cube(
        &self,
        state: &EBRState,
        from: &ChoosableAction,
        to: &ChoosableAction,
    ) -> EBRState {
        let mut state = state.clone();
        let Actor::Player(next_actor) = state.next_actor else {
            unreachable!()
        };
        state.active_player = next_actor;
        // Find index of cube to remove
        let remove_idx = state
            .action_cubes
            .iter()
            .enumerate()
            .find(|(i, &cube)| cube && ACTION_CUBE_SPACES[*i] == *from)
            .unwrap()
            .0;
        let add_idx = state
            .action_cubes
            .iter()
            .enumerate()
            .find(|(i, &cube)| !cube && ACTION_CUBE_SPACES[*i] == *to)
            .unwrap()
            .0;
        state.action_cubes[remove_idx] = false;
        state.action_cubes[add_idx] = true;
        match to {
            ChoosableAction::AuctionShare => state.stage = Stage::ChooseAuctionCompany,
            ChoosableAction::PayDividend => state.pay_dividend(),
            ChoosableAction::BuildTrack => state.stage = Stage::ChooseBuildCompany,
            ChoosableAction::IssueBond => state.stage = Stage::ChooseBondCompany,
            ChoosableAction::Merge => state.stage = Stage::ChooseMerge,
            ChoosableAction::TakeResources => state.stage = Stage::ChooseTakeResourcesCompany,
            _ => {} //warn!("Not implemented yet"),
        }
        state
    }

    fn execute_choose_auction_company(&self, state: &EBRState, company: &Company) -> EBRState {
        let mut state = state.clone();
        if !state.company_fixed_details[&company].private {
            state.stage = Stage::Auction {
                initial_auction: false,
                current_bid: None,
                lot: *company,
                winning_bidder: None,
                passed: HashSet::new(),
            };
        } else {
            state.stage = Stage::ChoosePrivateStart(*company);
        }
        state
    }

    fn execute_start_private_at(
        &self,
        state: &EBRState,
        company: &Company,
        location: &Coordinate,
    ) -> EBRState {
        let mut state = state.clone();
        state.company_details.get_mut(company).unwrap().hq = Some(*location);
        state.stage = Stage::Auction {
            initial_auction: false,
            current_bid: None,
            lot: *company,
            winning_bidder: None,
            passed: HashSet::new(),
        };
        if !state
            .track
            .iter()
            .any(|t| t.location == *location && t.track_type == TrackType::Narrow)
        {
            state.track.push(Track {
                location: *location,
                track_type: TrackType::Narrow,
            });
        }
        // Place resource cubes around
        let mut potential_locations = get_neighbors(location.clone()).to_vec();
        potential_locations.push(*location);
        for location in potential_locations {
            if location.0 >= WIDTH || location.1 >= HEIGHT {
                continue;
            }
            let terrain = TERRAIN[location.1][location.0];
            match terrain {
                Terrain::Forest => state.resource_cubes.push(location),
                Terrain::Mountain => {
                    state.resource_cubes.push(location);
                    state.resource_cubes.push(location);
                }
                _ => {}
            };
        }

        state
    }

    fn execute_choose_build_company(&self, state: &EBRState, company: &Company) -> EBRState {
        let mut state = state.clone();
        state.stage = Stage::BuildTrack {
            company: *company,
            completed_builds: 0,
        };
        state
    }

    fn execute_build_track(&self, state: &EBRState, location: &Coordinate) -> EBRState {
        let mut state = state.clone();
        if let Stage::BuildTrack {
            company,
            completed_builds,
        } = state.stage
        {
            let is_private = state.company_fixed_details[&company].private;

            let track_type;
            let cost;

            if is_private {
                track_type = TrackType::Narrow;
                cost = state.narrow_cost(*location) as isize;
                state.narrow_gauge_remaining -= 1;
            } else {
                track_type = TrackType::CompanyOwned(company.clone());
                cost = state.owned_cost(*location, None) as isize;
                state
                    .company_details
                    .get_mut(&company)
                    .unwrap()
                    .track_remaining -= 1;
            }

            state.track.push(Track {
                location: *location,
                track_type,
            });

            state.company_details.get_mut(&company).unwrap().cash -= cost;

            let Actor::Player(next_actor) = state.next_actor else {
                unreachable!()
            };
            if completed_builds < state.hyperparams.max_builds
                && state.can_build(company, next_actor)
            {
                state.stage = Stage::BuildTrack {
                    company,
                    completed_builds: completed_builds + 1,
                }
            } else {
                state.stage = Stage::ChooseAction;
                state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
            }
            state
        } else {
            unreachable!()
        }
    }

    fn execute_build_pass(&self, state: &EBRState) -> EBRState {
        let mut state = state.clone();
        state.stage = Stage::ChooseAction;
        state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
        state
    }

    fn execute_choose_bond_company(&self, state: &EBRState, company: &Company) -> EBRState {
        let mut state = state.clone();
        state.stage = Stage::ChooseBond(company.clone());
        state
    }

    fn execute_issue_bond(&self, state: &EBRState, company: &Company, bond: &Bond) -> EBRState {
        let mut state = state.clone();
        let details = state.company_details.get_mut(&company).unwrap();
        details.cash += bond.face_value as isize;
        details.bonds.push(BondDetails {
            bond: *bond,
            deferred: true,
        });
        state.unissued_bonds.retain(|b| *b != *bond);
        state.stage = Stage::ChooseAction;
        state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
        state
    }

    fn execute_merge(&self, state: &EBRState, private: &Company, company: &Company) -> EBRState {
        let mut state = state.clone();
        {
            let (private_cash, private_bonds) = {
                let private_details = state.company_details.get_mut(&private).unwrap();
                private_details.merged = Some(true);
                (private_details.cash, private_details.bonds.clone())
            };
            let company_details = state.company_details.get_mut(&company).unwrap();
            company_details.cash += private_cash;
            company_details.bonds.extend(private_bonds.clone());
            // TODO: Rules for share handling on merge are incomplete.
            // 1. When merging with EBRC, EBRC should get one of its reserved shares. This logic is missing.
            // 2. When merging with another company, one of EBRC's reserved shares should become available for auction. This is also missing.
            if company != &Company::EBRC {
                company_details.shares_held += 1;
                company_details.shares_remaining -= 1;
            }
            company_details.owned_privates.push(private.clone());
        }
        state.holdings = state
            .holdings
            .iter()
            .map(|(&player, companies)| {
                (
                    player,
                    companies
                        .iter()
                        .map(|c| {
                            if c != private {
                                c.clone()
                            } else {
                                company.clone()
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        state.stage = Stage::ChooseAction;
        state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
        state
    }

    fn execute_choose_take_resources_company(
        &self,
        state: &EBRState,
        company: &Company,
        delivery_company: &Option<Company>,
    ) -> EBRState {
        let mut state = state.clone();
        state.stage = Stage::TakeResources {
            company: *company,
            delivery_company: delivery_company.unwrap_or(*company),
            taken_resources: 0,
        };
        state
    }

    fn execute_take_resources(&self, state: &EBRState, coordinate: &Coordinate) -> EBRState {
        // ANNOTATION: Major rules discrepancy here.
        // Rules: Company pays ₤3 per cube, and its revenue track increases.
        // Implementation: Company pays nothing. Shareholders receive a small,
        // immediate cash dividend. The `TAKE_RESOURCE_COST` constant is unused.
        let mut state = state.clone();
        if let Stage::TakeResources {
            company,
            delivery_company,
            taken_resources,
        } = state.stage
        {
            state.resource_cubes.retain(|c| c != coordinate);

            {
                let mut new_cash = state.player_cash.clone();
                state.holdings.iter().for_each(|(&player, companies)| {
                    {
                        companies.iter().for_each(|c| {
                            if *c == company {
                                *new_cash.get_mut(&player).unwrap() +=
                                    state.hyperparams.take_dividend as isize;
                            }

                            if *c == delivery_company {
                                if state.has_port(delivery_company) {
                                    *new_cash.get_mut(&player).unwrap() +=
                                        state.hyperparams.take_port_deliver_dividend as isize;
                                } else if state.has_town(delivery_company) {
                                    *new_cash.get_mut(&player).unwrap() +=
                                        state.hyperparams.take_town_deliver_dividend as isize;
                                }
                            }
                        })
                    }
                });

                state.player_cash = new_cash;
            };

            state.stage = Stage::TakeResources {
                company,
                delivery_company,
                taken_resources: taken_resources + 1,
            }
        }
        state
    }

    fn execute_pass_take_resources(&self, state: &EBRState) -> EBRState {
        let mut state = state.clone();
        state.stage = Stage::ChooseAction;
        state.next_actor = Actor::Player((state.active_player + 1) % state.player_count);
        state
    }
}

impl Action for EBRAction {
    type StateType = EBRState;
    fn execute(&self, state: &Self::StateType) -> Self::StateType {
        match self {
            EBRAction::Stalemate => self.execute_stalemate(state),
            EBRAction::Bid(bid) => self.execute_bid(state, bid),
            EBRAction::Pass => self.execute_pass(state),
            EBRAction::MoveCube(from, to) => self.execute_move_cube(state, from, to),
            EBRAction::ChooseAuctionCompany(company) => {
                self.execute_choose_auction_company(state, company)
            }
            EBRAction::StartPrivateAt(company, location) => {
                self.execute_start_private_at(state, company, location)
            }
            EBRAction::ChooseBuildCompany(company) => {
                self.execute_choose_build_company(state, company)
            }
            EBRAction::BuildTrack(location) => self.execute_build_track(state, location),
            EBRAction::BuildPass => self.execute_build_pass(state),
            EBRAction::ChooseBondCompany(company) => {
                self.execute_choose_bond_company(state, company)
            }
            EBRAction::IssueBond(company, bond) => self.execute_issue_bond(state, company, bond),
            EBRAction::Merge(private, company) => self.execute_merge(state, private, company),
            EBRAction::ChooseTakeResourcesCompany(company, delivery_company) => {
                self.execute_choose_take_resources_company(state, company, delivery_company)
            }
            EBRAction::TakeResources(coordinate) => self.execute_take_resources(state, coordinate),
            EBRAction::PassTakeResources => self.execute_pass_take_resources(state),
        }
    }
}

type PlayerID = u8;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TrackType {
    CompanyOwned(Company),
    Narrow,
}

#[derive(Clone, Debug)]
struct Track {
    location: Coordinate,
    track_type: TrackType,
}

#[derive(Clone, Debug, PartialEq)]
enum Stage {
    Auction {
        initial_auction: bool,
        current_bid: Option<isize>,
        lot: Company,
        winning_bidder: Option<PlayerID>,
        passed: HashSet<PlayerID>,
    },
    BuildTrack {
        company: Company,
        completed_builds: usize,
    },
    ChooseAction,
    TakeResources {
        company: Company,
        delivery_company: Company,
        taken_resources: u8,
    },
    ChooseTakeResourcesCompany,
    ChooseAuctionCompany,
    ChoosePrivateStart(Company),
    ChooseBuildCompany,
    ChooseBondCompany,
    ChooseBond(Company),
    ChooseMerge,
}

#[derive(Clone, Debug)]
pub struct EBRState {
    terminal: bool,
    end_game_reason: EndGameReason,
    next_actor: Actor<EBRAction>,
    active_player: PlayerID,
    player_count: u8,
    track: Vec<Track>,
    stage: Stage,
    holdings: HashMap<PlayerID, Vec<Company>>,
    player_cash: HashMap<PlayerID, isize>,
    action_cubes: ActionCubeSpaces,
    revenue: HashMap<Company, isize>,
    dividends_paid: usize,
    company_details: HashMap<Company, CompanyDetails>,
    unissued_bonds: Vec<Bond>,
    resource_cubes: Vec<Coordinate>,
    narrow_gauge_remaining: usize,
    terrain_attributes: HashMap<Terrain, CommonAttributes>,
    company_fixed_details: HashMap<Company, CompanyFixedDetailsParams>,
    hyperparams: Arc<EBRHyperparams>,
    initial_auction_winners: HashMap<Company, PlayerID>,
}

impl EBRState {
    fn min_bid(&self, company: Company) -> isize {
        let rev = self.net_revenue(company.clone());
        let owned_shares = self.company_details[&company].shares_held;
        return max(1, div_ceil(rev, owned_shares as isize + 1));
    }

    fn can_auction_any(&self) -> bool {
        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };
        let cash = self.player_cash[&next_actor];
        if &cash < &1 {
            return false;
        };
        // Check for min bid of at least one company with shares available
        // (including the minors)
        self.company_fixed_details
            .iter()
            .any(|c| self.can_auction(c.0.clone(), cash))
    }

    fn can_auction(&self, company: Company, cash: isize) -> bool {
        let company_details = &self.company_details[&company];
        let private = self.company_fixed_details[&company].private;
        ((private
            && company_details
                .available
                .expect("Private Company Details Should Have Available"))
            || (!private && company_details.shares_remaining > 0))
            && (cash >= self.min_bid(company))
    }

    fn can_issue_any(&self) -> bool {
        if self.unissued_bonds.is_empty() {
            return false;
        }
        self.company_fixed_details
            .iter()
            .any(|c| self.can_issue(c.0.clone()))
    }
    fn can_issue(&self, company: Company) -> bool {
        // TODO: This check is incomplete. Rules require that the number of issued
        // bonds is less than the number of sold shares. This is not currently checked.
        if self.company_fixed_details[&company].private {
            return false;
        };

        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };

        self.holdings[&next_actor].contains(&company)
    }

    fn can_merge_any(&self) -> bool {
        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };

        self.merge_options(next_actor).len() > 0
    }

    fn merge_options(&self, player: PlayerID) -> BTreeSet<(Company, Company)> {
        self.holdings[&player]
            .iter()
            .map(|c| c.clone())
            .collect::<BTreeSet<Company>>()
            .iter()
            .filter(|c| {
                !self.company_fixed_details[&c].private
                    || (self.company_fixed_details[&c].private
                        && !self.company_details[&c].merged.unwrap_or(false))
            })
            .flat_map(|c| {
                if self.company_fixed_details[&c].private {
                    self.company_fixed_details
                        .iter()
                        .filter(|possible_public| {
                            !self.company_fixed_details[&possible_public.0].private
                        })
                        .map(|public_co| (c.clone(), public_co.0.clone()))
                        .collect::<Vec<(Company, Company)>>()
                } else {
                    self.company_fixed_details
                        .iter()
                        .filter(|possible_private| {
                            self.company_fixed_details[&possible_private.0].private
                                && !self.company_details[&possible_private.0]
                                    .merged
                                    .unwrap_or(false)
                        })
                        .map(|private_co| (private_co.0.clone(), c.clone()))
                        .collect()
                }
            })
            .collect::<BTreeSet<(Company, Company)>>()
            .iter()
            .filter(|(_private_co, public_co)| {
                self.company_details[public_co].shares_remaining > 0 ||
                                //TODO: Make the EBRC here data somewhere
                                *public_co == Company::EBRC
            })
            .map(|c| c.clone())
            .filter(
                // Check if actually connected
                // Left to last because slowest
                |(private_co, public_co)| self.connected_to(private_co.clone(), public_co.clone()),
            )
            .collect()
    }

    fn connected_to(&self, private_co: Company, public_co: Company) -> bool {
        let public_co_track = TrackType::CompanyOwned(public_co);

        self.reachable_narrow_track(private_co)
            .iter()
            .flat_map(|&t| get_neighbors(t))
            .any(|neighbor| {
                self.track
                    .iter()
                    .any(|ot| ot.location == neighbor && ot.track_type == public_co_track)
            })
    }

    fn connected_majors(&self, private_co: Company) -> Vec<Company> {
        self.company_fixed_details
            .iter()
            .filter(|c| !c.1.private)
            .filter(|public_c| self.connected_to(private_co, public_c.0.clone()))
            .map(|c| c.0.clone())
            .collect()
    }

    fn has_port(&self, company: Company) -> bool {
        self.track.iter().any(|t| {
            t.track_type == TrackType::CompanyOwned(company)
                && TERRAIN[t.location.1][t.location.0] == Terrain::Port
        })
    }
    fn has_town(&self, company: Company) -> bool {
        self.track.iter().any(|t| {
            t.track_type == TrackType::CompanyOwned(company)
                && TERRAIN[t.location.1][t.location.0] == Terrain::Town
        })
    }

    fn can_build_any(&self) -> bool {
        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };
        self.company_fixed_details
            .iter()
            .any(|c| self.can_build(c.0.clone(), next_actor))
    }

    fn possible_owned_track(&self, company: Company) -> Vec<Coordinate> {
        let company_details = self.company_details.get(&company).unwrap();
        self.track
            .iter()
            .filter(|t| {
                // All owned track
                t.track_type == TrackType::CompanyOwned(company.clone())
            })
            // All neighboring
            .map(|t| get_neighbors(t.location))
            .flatten()
            .collect::<HashSet<Coordinate>>() // Unique
            .iter()
            .filter(|t| t.0 < WIDTH && t.1 < HEIGHT)
            .filter_map(|t| {
                if t.0 >= WIDTH || t.1 >= HEIGHT {
                    return None;
                }
                let terrain = TERRAIN[t.1][t.0];
                let attr = &self.terrain_attributes[&terrain];
                if !attr.buildable {
                    return None;
                }
                let other_track_in_location = self
                    .track
                    .iter()
                    .map(|ot| ot.clone())
                    .filter(|ot| ot.location == *t)
                    .collect::<Vec<_>>();
                // Can't build more track if not permitted
                if other_track_in_location.len() > 0 && !attr.multiple_allowed {
                    return None;
                }
                // Company can't own multiple track in location
                if other_track_in_location
                    .iter()
                    .any(|t| t.track_type == TrackType::CompanyOwned(company.clone()))
                {
                    return None;
                }
                // Make sure co can pay
                let cost = self.owned_cost(*t, Some(other_track_in_location));
                if company_details.cash >= cost as isize {
                    Some(*t)
                } else {
                    None
                }
            })
            .collect()
    }

    fn owned_cost(&self, t: Coordinate, other_track_in_location: Option<Vec<Track>>) -> usize {
        // Other track in location is optional - only calculate if not specified
        let other_track_in_location = other_track_in_location.unwrap_or(
            self.track
                .iter()
                .filter_map(|ot| {
                    if ot.location == t {
                        Some(ot.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<Track>>(),
        );

        // Slight repetition of other places where this is called here
        let terrain = TERRAIN[t.1][t.0];

        let attr = &self.terrain_attributes[&terrain];
        (1 + other_track_in_location.len()) * attr.build_cost as usize
            + self
                .hyperparams
                .all_features()
                .get(&t)
                .iter()
                .map(|f| f.additional_cost)
                .sum::<usize>()
    }

    fn reachable_narrow_track(&self, company: Company) -> Vec<Coordinate> {
        // This might need to be cached
        if self.company_details[&company].hq.is_none() {
            return vec![];
        }
        let mut to_visit = HashSet::<Coordinate>::new();
        let mut visited = HashSet::<Coordinate>::new();
        to_visit.insert(self.company_details[&company].hq.unwrap());
        while to_visit.len() > 0 {
            let coord = to_visit.iter().next().unwrap().clone();
            let neighbors = get_neighbors(coord.clone());
            visited.insert(coord.clone());
            to_visit.remove(&coord);
            to_visit.extend(neighbors.iter().filter(|n| {
                !visited.contains(n)
                    && self
                        .track
                        .iter()
                        .any(|t| t.location == **n && t.track_type == TrackType::Narrow)
            }));
        }
        visited.iter().cloned().collect()
    }

    fn possible_narrow_track(&self, company: Company) -> Vec<Coordinate> {
        let cash = self.company_details[&company].cash;
        self.reachable_narrow_track(company)
            .iter()
            .map(|t| get_neighbors(*t))
            .flatten()
            .filter(|t| t.0 < WIDTH && t.1 < HEIGHT)
            .filter(|t| {
                !(self.narrow_cost(*t) as isize > cash
                    && !self.track.iter().any(|t2| t2.location == *t))
                    && self.terrain_attributes[&TERRAIN[t.1][t.0]].buildable
            })
            .collect::<BTreeSet<_>>()
            .iter()
            .map(|t| t.clone())
            .collect()
    }

    fn narrow_cost(&self, _t: Coordinate) -> usize {
        return self.hyperparams.narrow_track_cost;
    }

    fn can_build(&self, company: Company, player: PlayerID) -> bool {
        let company_details = self.company_details.get(&company).unwrap();
        if !self.holdings.get(&player).unwrap().contains(&company) {
            return false;
        }
        if company_details.merged.unwrap_or(false) {
            return false;
        }
        let company_fixed_details = self.company_fixed_details.get(&company).unwrap();
        if !company_fixed_details.private {
            if company_fixed_details.track_available == 0 {
                return false;
            }
            self.possible_owned_track(company).len() > 0
        } else {
            if self.narrow_gauge_remaining == 0 {
                return false;
            }
            self.possible_narrow_track(company).len() > 0
        }
    }

    fn can_take_any(&self) -> bool {
        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };
        self.holdings[&next_actor]
            .iter()
            .collect::<HashSet<_>>()
            .iter()
            .any(|p| self.can_take(**p))
    }

    fn can_take(&self, company: Company) -> bool {
        (self.company_details[&company].cash > self.hyperparams.take_resource_cost as isize)
            && self.company_accessible_resources(company).len() > 0
    }

    fn company_accessible_resources(&self, company: Company) -> Vec<Coordinate> {
        // Major: Anything in space of track or narrow connected to owned minor
        // Minor: Anything connected to narrow
        let company_details = self.company_details.get(&company).unwrap();
        let accessible_spaces = if self.company_fixed_details[&company].private {
            let mut spaces = self.possible_owned_track(company.clone());
            spaces.extend(
                company_details
                    .owned_privates
                    .iter()
                    .flat_map(|p| self.reachable_narrow_track(p.clone()))
                    .collect::<Vec<Coordinate>>(),
            );
            spaces
        } else {
            self.possible_narrow_track(company)
        };
        let accessible_spaces = accessible_spaces.iter().collect::<HashSet<_>>();

        self.resource_cubes
            .iter()
            .filter(|r| accessible_spaces.contains(r))
            .map(|coord| *coord)
            .collect()
    }

    fn net_revenue(&self, company: Company) -> isize {
        let company_track = self
            .track
            .iter()
            .filter(|t| t.track_type == TrackType::CompanyOwned(company.clone()));
        let track_terrain_revenue = company_track
            .clone()
            .map(|t| {
                self.terrain_attributes[&TERRAIN[t.location.1][t.location.0]].revenue
                    [self.dividends_paid]
            })
            .sum::<isize>();
        let track_feature_revenue = company_track
            .clone()
            .map(|t| {
                match self
                    .hyperparams
                    .all_features()
                    .get_key_value(&(t.location.0, t.location.1))
                {
                    None => 0,
                    Some(feature) => feature.1.revenue[self.dividends_paid],
                }
            })
            .sum::<isize>();
        // FIXME: Bond interest logic is non-functional.
        // A bond is issued as `deferred = true`. The `pay_dividend` function also sets
        // `deferred = true`. This means a bond's `deferred` status is never `false`,
        // so its `coupon` value is never actually subtracted from revenue here.
        let bond_interest = self
            .company_details
            .get(&company)
            .unwrap()
            .bonds
            .iter()
            .filter_map(|b| {
                if b.deferred {
                    None
                } else {
                    Some(b.bond.coupon)
                }
            })
            .sum::<usize>();
        track_terrain_revenue + track_feature_revenue - bond_interest as isize
    }

    fn pay_dividend(&mut self) {
        let rev_per_share = self
            .company_details
            .iter()
            .map(|c| {
                (
                    c.0.clone(),
                    if c.1.shares_held > 0 {
                        let rev = self.net_revenue(c.0.clone());
                        // Ceil over 0, floor under 0
                        if rev > 0 {
                            div_ceil(rev, c.1.shares_held as isize)
                        } else {
                            div_ceil(rev * -1, c.1.shares_held as isize) * -1
                        }
                    } else {
                        0
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        self.next_actor = {
            let Actor::Player(actor) = self.next_actor else {
                unreachable!()
            };
            Actor::Player((&actor + 1) % self.player_count)
        };
        self.player_cash = self
            .player_cash
            .iter()
            .map(|(player, old_cash)| {
                (
                    *player,
                    old_cash
                        + self.holdings[player]
                            .iter()
                            .map(|company| rev_per_share[company])
                            .sum::<isize>(),
                )
            })
            .collect::<HashMap<u8, isize>>();

        // ANNOTATION: This loop updates the state of bonds for the next round.
        // Any bond that was deferred for this dividend payment will now be flipped
        // to `deferred = false`, so its interest will be paid in all future rounds.
        for company in self.company_details.values_mut() {
            for bond in company.bonds.iter_mut() {
                bond.deferred = false;
            }
        }
        self.dividends_paid += 1;

        // ANNOTATION: End-game checks. The game can end due to running out of dividend
        // rounds, player bankruptcy, or by meeting 2 of the 4 specified conditions.
        let shares_condition = self
            .company_details
            .iter()
            .filter(|c| !c.1.merged.unwrap_or(false) && c.1.shares_remaining > 0)
            .count()
            == 0;
        let bonds_condition = self.unissued_bonds.len() <= 2;
        let resources_condition = self.resource_cubes.len() <= 3;

        let end_conditions_met =
            (shares_condition as u8) + (bonds_condition as u8) + (resources_condition as u8);

        if self.dividends_paid == FINAL_DIVIDEND_COUNT {
            self.terminal = true;
            self.end_game_reason = EndGameReason::Dividends;
        } else if self.player_cash.iter().any(|(_, cash)| *cash < 0) {
            self.terminal = true;
            self.end_game_reason = EndGameReason::Bankruptcy;
        } else if end_conditions_met >= 2 {
            self.terminal = true;
            // TODO: This doesn't correctly capture the case where multiple conditions are met
            if shares_condition {
                self.end_game_reason = EndGameReason::Shares;
            } else if bonds_condition {
                self.end_game_reason = EndGameReason::Bonds;
            } else if resources_condition {
                self.end_game_reason = EndGameReason::Resources;
            }
        }
    }
}

impl State for EBRState {
    type ActionType = EBRAction;
    type GameHyperrewardType = EBRHyperrewards;

    fn next_actor(&self) -> Actor<EBRAction> {
        self.next_actor.clone()
    }
    fn round_hyperreward(&self) -> Self::GameHyperrewardType {
        let devonport = (7, 3);
        let launceston = (9, 4);
        let hobart = (10, 9);

        let is_connected = |company: &Company, location: Coordinate| {
            if self.company_fixed_details[company].private {
                self.reachable_narrow_track(*company).contains(&location)
            } else {
                if self.company_details[company].hq.is_none() {
                    return false;
                }
                let hq = self.company_details[company].hq.unwrap();
                let company_track_type = TrackType::CompanyOwned(*company);
                if !self
                    .track
                    .iter()
                    .any(|t| t.location == hq && t.track_type == company_track_type)
                {
                    return false;
                }

                let mut to_visit = vec![hq];
                let mut visited = HashSet::new();

                while let Some(coord) = to_visit.pop() {
                    if !visited.insert(coord) {
                        continue;
                    }
                    if coord == location {
                        return true;
                    }
                    for neighbor in get_neighbors(coord) {
                        if !visited.contains(&neighbor)
                            && self.track.iter().any(|t| {
                                t.location == neighbor && t.track_type == company_track_type
                            })
                        {
                            to_visit.push(neighbor);
                        }
                    }
                }
                false
            }
        };

        let mut sorted_cash: Vec<(u8, isize)> = self
            .player_cash
            .iter()
            .map(|(player, cash)| (*player, *cash))
            .collect();
        sorted_cash.sort_by(|a, b| b.1.cmp(&a.1));

        let (winning_player_id, winning_player_score) = if self.terminal {
            (Some(sorted_cash[0].0 as usize), Some(sorted_cash[0].1))
        } else {
            (None, None)
        };

        let all_track_locations: HashSet<Coordinate> =
            self.track.iter().map(|t| t.location).collect();
        let mut terrain_coords: HashMap<Terrain, Vec<Coordinate>> = HashMap::new();
        for (y, row) in TERRAIN.iter().enumerate() {
            for (x, &terrain) in row.iter().enumerate() {
                terrain_coords.entry(terrain).or_default().push((x, y));
            }
        }

        let mut terrain_track_ratios: HashMap<Terrain, f32> = HashMap::new();
        for (terrain, coords) in &terrain_coords {
            if *terrain == Terrain::Nothing {
                continue;
            }
            let tracked_count = coords
                .iter()
                .filter(|c| all_track_locations.contains(c))
                .count();
            let ratio = if coords.is_empty() {
                0.0
            } else {
                tracked_count as f32 / coords.len() as f32
            };
            terrain_track_ratios.insert(*terrain, ratio);
        }

        let non_nothing_tiles: Vec<Coordinate> = terrain_coords
            .iter()
            .filter(|(&t, _)| t != Terrain::Nothing)
            .flat_map(|(_, coords)| coords)
            .cloned()
            .collect();

        let tracked_non_nothing_count = non_nothing_tiles
            .iter()
            .filter(|c| all_track_locations.contains(c))
            .count();

        let overall_track_ratio = if non_nothing_tiles.is_empty() {
            0.0
        } else {
            tracked_non_nothing_count as f32 / non_nothing_tiles.len() as f32
        };

        EBRHyperrewards {
            total_bonds_issued: self.hyperparams.bonds.len() - self.unissued_bonds.len(),
            end_game_reason: self.end_game_reason,
            remaining_resource_cubes: self.resource_cubes.len(),
            ebrc_connected_to_devonport: is_connected(&Company::EBRC, devonport),
            ebrc_connected_to_launceston: is_connected(&Company::EBRC, launceston),
            ebrc_connected_to_hobart: is_connected(&Company::EBRC, hobart),
            lw_connected_to_devonport: is_connected(&Company::LW, devonport),
            lw_connected_to_launceston: is_connected(&Company::LW, launceston),
            lw_connected_to_hobart: is_connected(&Company::LW, hobart),
            tmlc_connected_to_devonport: is_connected(&Company::TMLC, devonport),
            tmlc_connected_to_launceston: is_connected(&Company::TMLC, launceston),
            tmlc_connected_to_hobart: is_connected(&Company::TMLC, hobart),
            gt_connected_to_devonport: is_connected(&Company::GT, devonport),
            gt_connected_to_launceston: is_connected(&Company::GT, launceston),
            gt_connected_to_hobart: is_connected(&Company::GT, hobart),
            nmft_connected_to_devonport: is_connected(&Company::NMFT, devonport),
            nmft_connected_to_launceston: is_connected(&Company::NMFT, launceston),
            nmft_connected_to_hobart: is_connected(&Company::NMFT, hobart),
            ned_connected_to_devonport: is_connected(&Company::NED, devonport),
            ned_connected_to_launceston: is_connected(&Company::NED, launceston),
            ned_connected_to_hobart: is_connected(&Company::NED, hobart),
            mlm_connected_to_devonport: is_connected(&Company::MLM, devonport),
            mlm_connected_to_launceston: is_connected(&Company::MLM, launceston),
            mlm_connected_to_hobart: is_connected(&Company::MLM, hobart),
            completed_dividend_rounds: self.dividends_paid,
            gt_merged: self.company_details[&Company::GT].merged.unwrap_or(false),
            nmft_merged: self.company_details[&Company::NMFT].merged.unwrap_or(false),
            ned_merged: self.company_details[&Company::NED].merged.unwrap_or(false),
            mlm_merged: self.company_details[&Company::MLM].merged.unwrap_or(false),
            lw_auction_winner: self
                .initial_auction_winners
                .get(&Company::LW)
                .map(|id| *id as usize),
            tmlc_auction_winner: self
                .initial_auction_winners
                .get(&Company::TMLC)
                .map(|id| *id as usize),
            ebrc_auction_winner: self
                .initial_auction_winners
                .get(&Company::EBRC)
                .map(|id| *id as usize),
            gt_auction_winner: self
                .initial_auction_winners
                .get(&Company::GT)
                .map(|id| *id as usize),
            winning_player_id,
            winning_player_score,
            player_scores: sorted_cash.iter().map(|s| s.1).collect(),
            overall_track_ratio,
            terrain_track_ratios,
        }
    }

    fn permitted_actions(&self) -> Vec<Self::ActionType> {
        let Actor::Player(next_actor) = self.next_actor else {
            unreachable!()
        };
        if self.terminal {
            return vec![];
        }
        match &self.stage {
            Stage::Auction {
                initial_auction,
                current_bid,
                ..
            } => {
                let player_cash = *self.player_cash.get(&next_actor).unwrap();
                if (current_bid.unwrap_or(-1) as isize) < player_cash {
                    let mut actions: Vec<EBRAction> = (((current_bid.unwrap_or(0) + 1) as isize)
                        ..=player_cash)
                        .map(|bid| EBRAction::Bid(bid as usize))
                        .collect();
                    if *initial_auction && (*current_bid == None) {
                        actions.push(EBRAction::Bid(0));
                    } else if (!(*initial_auction) && *current_bid != None)
                        || (*current_bid != None)
                    {
                        actions.push(EBRAction::Pass);
                    }
                    actions
                } else {
                    vec![if *initial_auction && (*current_bid == None) {
                        EBRAction::Bid(0)
                    } else if !(*initial_auction) || (*current_bid != None) {
                        EBRAction::Pass
                    } else {
                        panic!("Somehow, Palapatine has returned")
                    }]
                }
            }
            Stage::ChooseAction => {
                let removable_action_cubes = self
                    .action_cubes
                    .iter()
                    .enumerate()
                    .filter(|(_, &cube)| cube)
                    .map(|(i, _)| ACTION_CUBE_SPACES[i])
                    // BTreeSet as wanted the order, and perf was worth it
                    .collect::<BTreeSet<ChoosableAction>>();
                let mut addable_action_cubes = self
                    .action_cubes
                    .iter()
                    .enumerate()
                    .filter(|(_, &cube)| !cube)
                    .map(|(i, _)| ACTION_CUBE_SPACES[i])
                    .collect::<BTreeSet<ChoosableAction>>();
                if !self.can_merge_any() {
                    addable_action_cubes.remove(&ChoosableAction::Merge);
                };
                if !self.can_build_any() {
                    addable_action_cubes.remove(&ChoosableAction::BuildTrack);
                }
                if !self.can_take_any() {
                    addable_action_cubes.remove(&ChoosableAction::TakeResources);
                }
                if !self.can_issue_any() {
                    addable_action_cubes.remove(&ChoosableAction::IssueBond);
                }
                if !self.can_auction_any() {
                    addable_action_cubes.remove(&ChoosableAction::AuctionShare);
                }

                let mut actions: Vec<EBRAction> = vec![];
                for remove_action in &removable_action_cubes {
                    for add_action in &addable_action_cubes {
                        if remove_action != add_action {
                            actions.push(EBRAction::MoveCube(*remove_action, *add_action));
                        }
                    }
                }
                if actions.is_empty() {
                    vec![EBRAction::Stalemate]
                } else {
                    actions
                }
            }
            Stage::ChooseAuctionCompany => {
                let cash = self.player_cash[&next_actor];
                self.company_fixed_details
                    .iter()
                    .filter(|c| self.can_auction(c.0.clone(), cash))
                    .map(|c| EBRAction::ChooseAuctionCompany(c.0.clone()))
                    .collect()
            }
            Stage::ChoosePrivateStart(company) => PRIVATE_STARTING_LOCATIONS
                .iter()
                .filter(|location| {
                    !self
                        .company_details
                        .iter()
                        .any(|c| c.1.hq == Some(**location))
                })
                .map(|location| EBRAction::StartPrivateAt(*company, *location))
                .collect(),
            Stage::ChooseBuildCompany => self
                .company_fixed_details
                .iter()
                .filter(|c| self.can_build(c.0.clone(), next_actor))
                .map(|c| EBRAction::ChooseBuildCompany(c.0.clone()))
                .collect(),
            Stage::BuildTrack {
                company,
                completed_builds,
            } => {
                if self.company_fixed_details[company].private {
                    if self.narrow_gauge_remaining == 0 {
                        return vec![EBRAction::BuildPass];
                    }
                    let mut actions = self
                        .possible_narrow_track(*company)
                        .iter()
                        .map(|coord| EBRAction::BuildTrack(*coord))
                        .collect::<Vec<EBRAction>>();
                    if *completed_builds > 0 {
                        actions.push(EBRAction::BuildPass)
                    };
                    actions
                } else {
                    if self.company_details[company].track_remaining == 0 {
                        return vec![EBRAction::BuildPass];
                    }
                    let mut actions = self
                        .possible_owned_track(*company)
                        .iter()
                        .map(|coord| EBRAction::BuildTrack(*coord))
                        .collect::<Vec<EBRAction>>();
                    if *completed_builds > 0 {
                        actions.push(EBRAction::BuildPass)
                    };
                    actions
                }
            }
            Stage::ChooseBondCompany => self
                .company_fixed_details
                .iter()
                .filter(|c| self.can_issue(c.0.clone()))
                .map(|c| EBRAction::ChooseBondCompany(c.0.clone()))
                .collect(),
            Stage::ChooseBond(company) => self
                .unissued_bonds
                .iter()
                .map(|bond| EBRAction::IssueBond(*company, *bond))
                .collect(),
            Stage::ChooseMerge => self
                .merge_options(next_actor)
                .iter()
                .map(|(private, company)| EBRAction::Merge(*private, *company))
                .collect(),
            Stage::ChooseTakeResourcesCompany => self
                .company_fixed_details
                .iter()
                .filter(|c| self.can_take(c.0.clone()))
                .flat_map(|c| {
                    let delivery_majors = self
                        .company_details
                        .iter()
                        .filter(|(major, _)| self.has_port(**major) || self.has_town(**major))
                        .collect::<Vec<_>>();
                    if delivery_majors.len() > 0 {
                        delivery_majors
                            .iter()
                            .map(|major| {
                                EBRAction::ChooseTakeResourcesCompany(
                                    c.0.clone(),
                                    Some(major.0.clone()),
                                )
                            })
                            .collect::<Vec<EBRAction>>()
                    } else {
                        vec![EBRAction::ChooseTakeResourcesCompany(c.0.clone(), None)]
                    }
                })
                .collect(),
            Stage::TakeResources {
                company,
                delivery_company,
                taken_resources,
            } => {
                let mut actions = self
                    .company_accessible_resources(*company)
                    .iter()
                    .map(|coord| EBRAction::TakeResources(*coord))
                    .collect::<Vec<EBRAction>>();
                if *taken_resources > 0 {
                    actions.push(EBRAction::PassTakeResources)
                };
                actions
            }
            _ => {
                warn!("Unimplemented Stage in PermittedActions");
                vec![]
            }
        }
    }

    fn reward(&self) -> Vec<f64> {
        // TODO: Improve this - this isn't great. 1 for best, -1 for lost, 0 for others.
        if !self.terminal {
            return vec![0f64; self.player_count as usize];
        }
        let mut cash_rewards = vec![0f64; self.player_count as usize];
        let mut sorted_cash: Vec<(u8, isize)> = self
            .player_cash
            .iter()
            .map(|(player, cash)| (*player, *cash))
            .collect();
        sorted_cash.sort_by(|a, b| b.1.cmp(&a.1));
        cash_rewards[sorted_cash[0].0 as usize] = 1f64;
        if self.player_count > 1 {
            cash_rewards[sorted_cash[self.player_count as usize - 1].0 as usize] = -1f64;
        }
        cash_rewards
    }

    fn terminal(&self) -> bool {
        self.terminal
    }
}

pub struct EBR {
    pub player_count: u8,
}

impl Game for EBR {
    type StateType = EBRState;
    type ActionType = EBRAction;
    type HyperparamsType = EBRHyperparams;
    type HyperrewardsType = EBRHyperrewards;

    fn init_game(&self, hyperparams: &Self::HyperparamsType) -> Self::StateType {
        let mut terrain_attributes = HashMap::new();
        for (terrain, params) in &hyperparams.terrain_attributes {
            let (buildable, multiple_allowed) = match terrain {
                Terrain::Nothing => (false, false),
                Terrain::Plain => (true, true),
                Terrain::Forest => (true, false),
                Terrain::Mountain => (true, false),
                Terrain::Town => (true, true),
                Terrain::Port => (true, true),
            };
            terrain_attributes.insert(
                *terrain,
                CommonAttributes {
                    build_cost: params.build_cost,
                    buildable,
                    multiple_allowed,
                    revenue: params.revenue,
                },
            );
        }

        // ANNOTATION: This function initializes the game to a fixed state for testing,
        // deviating from the random setup described in `Rules.md`.
        // - Player cash is hardcoded to `24 / player_count`.
        // - Initial track and resource cubes are from `INITIAL_TRACK` and `INITIAL_RESOURCE_CUBES`.
        // - Initial company revenue is 0, whereas the rules state it should start at 3.
        let company_fixed_details = hyperparams.company_fixed_details.clone();
        EBRState {
            terminal: false,
            end_game_reason: EndGameReason::InProgress,
            next_actor: Actor::Player(0),
            player_count: self.player_count,
            track: INITIAL_TRACK.to_vec(),
            active_player: 0,
            stage: Stage::Auction {
                initial_auction: true,
                current_bid: None,
                lot: Company::LW,
                winning_bidder: None,
                passed: HashSet::new(),
            },
            holdings: (0..self.player_count)
                .map(|i| (i, Vec::new()))
                .collect::<HashMap<u8, Vec<Company>>>(),
            player_cash: (0..self.player_count)
                .map(|i| {
                    (
                        i,
                        *hyperparams
                            .initial_cash
                            .get(&self.player_count)
                            .unwrap_or(&0) as isize,
                    )
                })
                .collect::<HashMap<u8, isize>>(),
            revenue: ALL_COMPANIES.iter().map(|c| (c.clone(), 0)).collect(),
            action_cubes: ACTION_CUBE_INIT,
            dividends_paid: 0,
            company_details: company_fixed_details
                .iter()
                .map(|d| {
                    (
                        d.0.clone(),
                        CompanyDetails {
                            shares_held: 0,
                            shares_remaining: d.1.stock_available,
                            merged: if d.1.private { Some(false) } else { None },
                            cash: d.1.initial_treasury as isize,
                            available: if d.1.private { Some(false) } else { None },
                            hq: d.1.starting,
                            track_remaining: d.1.track_available,
                            bonds: vec![BondDetails {
                                bond: Bond {
                                    face_value: d.1.initial_treasury,
                                    coupon: d.1.initial_interest,
                                },
                                deferred: true,
                            }],
                            owned_privates: vec![],
                        },
                    )
                })
                .collect(),
            unissued_bonds: hyperparams
                .bonds
                .iter()
                .map(|b| b.clone())
                .collect::<Vec<Bond>>(),
            resource_cubes: hyperparams.initial_resource_cubes.to_vec(),
            narrow_gauge_remaining: hyperparams.narrow_gauge_initial,
            terrain_attributes,
            company_fixed_details,
            hyperparams: Arc::new(hyperparams.clone()),
            initial_auction_winners: HashMap::new(),
        }
    }

    fn visualise_state(&self, state: &Self::StateType) {
        println!("Track:");
        for track in &state.track {
            println!("{:?}", track);
        }
        println!("Stage: {:?}", state.stage);
        println!("Active player: {}", state.active_player);
        println!("Player count: {}", state.player_count);
        println!("{:?}", state);
    }
}

fn div_ceil(numerator: isize, denominator: isize) -> isize {
    // Slightly cheeky
    // Look - it's used enough places that it's worth it, and frankly, it's clearer like this
    (numerator + denominator - 1) / denominator
}

/// Game is a hex map with pointy sides
/// Each row is top, bottom, top, bottom
///
/// 1,1        3, 1       5,1
///      2,1        4, 1
/// 1,2        3, 2,      5,2
///      2,2        4, 2
/// 1,3        3, 3       5,3
///      2,3        4, 3
/// This doesn't take into account the map
fn get_neighbors(coord: Coordinate) -> [Coordinate; 6] {
    let (x, y) = coord;
    if x % 2 == 1 {
        [
            (x, y - 1),
            (x + 1, y - 1),
            (x + 1, y),
            (x, y + 1),
            (x - 1, y),
            (x - 1, y - 1),
        ]
    } else {
        [
            (x, y - 1),
            (x + 1, y),
            (x + 1, y + 1),
            (x, y + 1),
            (x - 1, y + 1),
            (x - 1, y),
        ]
    }
}

mod test {

    use super::*;

    fn init_game() -> EBRState {
        let game = EBR { player_count: 3 };
        let hyperparams = EBRHyperparams::default();
        game.init_game(&hyperparams)
    }

    #[test]
    fn test_div_ceil() {
        assert_eq!(div_ceil(10, 3), 4);
        assert_eq!(div_ceil(10, 4), 3);
        assert_eq!(div_ceil(10, 5), 2);
    }

    #[test]
    fn test_connected_to() {
        // Test will break if HQ of GT or EBRC moved
        let mut game_state = init_game();

        // Assert GT initially connected to EBRC
        assert!(game_state.connected_to(Company::GT, Company::EBRC));
        // And not initially connected to LW
        assert!(!game_state.connected_to(Company::GT, Company::LW));
        // But connected if we build some track between them
        game_state.track.push(Track {
            location: (2, 3),
            track_type: TrackType::Narrow,
        });
        game_state.track.push(Track {
            location: (3, 3),
            track_type: TrackType::Narrow,
        });
        game_state.track.push(Track {
            location: (4, 3),
            track_type: TrackType::Narrow,
        });
        game_state.track.push(Track {
            location: (5, 3),
            track_type: TrackType::Narrow,
        });
        game_state.track.push(Track {
            location: (6, 3),
            track_type: TrackType::CompanyOwned(Company::LW),
        });
        game_state.track.push(Track {
            location: (7, 3),
            track_type: TrackType::CompanyOwned(Company::LW),
        });
        game_state.track.push(Track {
            location: (8, 3),
            track_type: TrackType::CompanyOwned(Company::LW),
        });
        assert!(game_state.connected_to(Company::GT, Company::LW));
    }

    #[test]
    fn test_reachable_narrow_track() {
        // Test will break if HQ of GT moved
        let mut game_state = init_game();

        // Check GT has its HQ initially
        assert!(
            game_state.reachable_narrow_track(Company::GT)
                == vec![game_state.company_fixed_details[&Company::GT]
                    .starting
                    .unwrap()]
        );

        // Check that nearby track not connected
        game_state.track.push(Track {
            location: (4, 4),
            track_type: TrackType::Narrow,
        });
        assert!(
            game_state.reachable_narrow_track(Company::GT)
                == vec![game_state.company_fixed_details[&Company::GT]
                    .starting
                    .unwrap()]
        );

        // Check that once connected, all three are there
        game_state.track.push(Track {
            location: (3, 4),
            track_type: TrackType::Narrow,
        });
        assert!(
            game_state
                .reachable_narrow_track(Company::GT)
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>()
                == vec![
                    game_state.company_fixed_details[&Company::GT]
                        .starting
                        .unwrap(),
                    (3, 4),
                    (4, 4)
                ]
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>()
        );
    }

    #[test]
    fn test_get_neighbors() {
        let expected1 = vec![(1, 4), (2, 3), (3, 4), (3, 5), (2, 5), (1, 5)];
        let actual1 = get_neighbors((2, 4));
        assert_eq!(
            expected1
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>(),
            actual1
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>()
        );

        let expected2 = vec![(2, 4), (2, 3), (3, 3), (4, 3), (4, 4), (3, 5)];
        let actual2 = get_neighbors((3, 4));
        assert_eq!(
            expected2
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>(),
            actual2
                .iter()
                .map(|t| t.clone())
                .collect::<HashSet<Coordinate>>()
        );
    }
}
