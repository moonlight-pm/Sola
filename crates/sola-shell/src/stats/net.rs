//! Network sampling (filled in Phase 5).

#[derive(Clone, Debug, Default)]
pub struct NetDetail;

#[derive(Clone, Debug, Default)]
pub struct Counters;

pub fn read_counters() -> Counters {
    Counters
}

pub fn rate(_prev: &Counters, _cur: &Counters, _dt: f32) -> (f32, f32) {
    (0.0, 0.0)
}

pub fn detail(_cur: &Counters) -> NetDetail {
    NetDetail
}
