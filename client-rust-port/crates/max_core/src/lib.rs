//! max_core — чистая игровая модель M.A.X. без I/O.
//!
//! Крейт описывает состояние партии и правила игры так, как их понимает клиент:
//! мир, команды, юниты, приказы. Никакой сети и никакого рендера. Состояние
//! приходит от сервера (снапшоты/дельты), приказы уходят на сервер; локально
//! модель используется для отображения, интерполяции и предпросмотра.

pub mod map;
pub mod order;
pub mod team;
pub mod unit;

pub use map::Map;
pub use order::Order;
pub use team::{Team, TeamId, TeamKind};
pub use unit::{Unit, UnitId, UnitKind};

/// Идентификатор партии/сессии Horizon.
pub type SessionId = uuid_like::Id;

/// Внутренний модуль с минимальным UUID-типом без внешней зависимости.
mod uuid_like {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
    pub struct Id(pub u128);

    impl std::fmt::Display for Id {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:032X}", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn core_smoke() {
        let team = crate::Team::new(crate::TeamId(0), crate::TeamKind::Player);
        assert_eq!(team.kind, crate::TeamKind::Player);
    }
}
