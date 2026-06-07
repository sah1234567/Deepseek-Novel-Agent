/// Inclusive turn_number range for transcript pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnBounds {
    pub min_turn: i32,
    pub max_turn: i32,
}

impl TurnBounds {
    pub fn new(min_turn: i32, max_turn: i32) -> Self {
        Self { min_turn, max_turn }
    }
}
