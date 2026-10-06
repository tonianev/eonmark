//! Schema for `data/rules/units.ron`: the unit kinds and their movement stats.
//!
//! M1 ships one kind, `yeoman` at index 0. Combat stats, costs and trainer
//! buildings arrive in M3a and M4a as further fields with their own
//! validator rules. Every value is an integer; fields ending in `_x100` are
//! scaled by 100 so that `180` means `1.80`. The sim converts them to
//! fixed-point exactly once at spawn time.

use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One unit kind. Index in [`Units::units`] is the `UnitKindId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitKind {
    /// Stable identifier other data files use (`yeoman`, ...).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Movement speed in tiles per second, times 100 (`180` is 1.8 tiles/s).
    pub speed_tiles_per_s_x100: u32,
    /// Collision radius in tiles, times 100 (`35` is 0.35 tiles).
    pub radius_tiles_x100: u32,
    /// A unit has arrived when it is within this radius of its final
    /// waypoint, in tiles times 100.
    pub arrive_radius_tiles_x100: u32,
    /// Arrival steering slows the unit linearly inside this radius around its
    /// final waypoint, in tiles times 100. At least `arrive_radius_tiles_x100`.
    pub arrive_slowdown_radius_tiles_x100: u32,
    /// A non-final waypoint counts as reached within this radius, in tiles
    /// times 100 (`50` is half a tile).
    pub waypoint_radius_tiles_x100: u32,
    /// Extra margin added to the sum of two radii before separation pushes
    /// neighbours apart, in tiles times 100.
    pub separation_tiles_x100: u32,
}

/// Contents of `data/rules/units.ron`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Units {
    /// Unit kinds in `UnitKindId` order.
    pub units: Vec<UnitKind>,
}

/// Upper bound on every `_x100` field so the sim's fixed-point conversion
/// (`value / (100 * tick_rate_hz)`) never leaves `i32` range.
pub const MAX_X100: u32 = 100_000;

impl Units {
    /// Validate ids and ranges. Every error names `path` and the field.
    pub fn validate(&self, path: &Path) -> Result<(), Error> {
        if self.units.is_empty() {
            return Err(Error::invalid(
                path,
                "units",
                "must list at least one unit kind",
            ));
        }
        if self.units.len() > usize::from(u16::MAX) {
            return Err(Error::invalid(
                path,
                "units",
                "more kinds than UnitKindId (u16) can index",
            ));
        }
        for (i, u) in self.units.iter().enumerate() {
            let field = |name: &str| format!("units[{i}].{name}");
            if u.id.is_empty() || !u.id.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                return Err(Error::invalid(
                    path,
                    &field("id"),
                    format!(
                        "{:?} must be non-empty lowercase ascii with underscores",
                        u.id
                    ),
                ));
            }
            if self.units[..i].iter().any(|o| o.id == u.id) {
                return Err(Error::invalid(
                    path,
                    &field("id"),
                    format!("duplicate id {:?}", u.id),
                ));
            }
            if u.name.is_empty() {
                return Err(Error::invalid(path, &field("name"), "must not be empty"));
            }
            let positive = |name: &str, v: u32| -> Result<(), Error> {
                if v == 0 {
                    return Err(Error::invalid(path, &field(name), "must be > 0"));
                }
                if v > MAX_X100 {
                    return Err(Error::invalid(
                        path,
                        &field(name),
                        format!("must be <= {MAX_X100}"),
                    ));
                }
                Ok(())
            };
            positive("speed_tiles_per_s_x100", u.speed_tiles_per_s_x100)?;
            positive("radius_tiles_x100", u.radius_tiles_x100)?;
            positive("waypoint_radius_tiles_x100", u.waypoint_radius_tiles_x100)?;
            let bounded = |name: &str, v: u32| -> Result<(), Error> {
                if v > MAX_X100 {
                    return Err(Error::invalid(
                        path,
                        &field(name),
                        format!("must be <= {MAX_X100}"),
                    ));
                }
                Ok(())
            };
            bounded("arrive_radius_tiles_x100", u.arrive_radius_tiles_x100)?;
            bounded(
                "arrive_slowdown_radius_tiles_x100",
                u.arrive_slowdown_radius_tiles_x100,
            )?;
            if u.arrive_slowdown_radius_tiles_x100 < u.arrive_radius_tiles_x100 {
                return Err(Error::invalid(
                    path,
                    &field("arrive_slowdown_radius_tiles_x100"),
                    "must be >= arrive_radius_tiles_x100",
                ));
            }
            bounded("separation_tiles_x100", u.separation_tiles_x100)?;
        }
        Ok(())
    }

    /// Look up a kind by id string.
    pub fn get(&self, id: &str) -> Option<&UnitKind> {
        self.units.iter().find(|u| u.id == id)
    }

    /// Look up a kind by index (`UnitKindId.0`).
    pub fn kind(&self, index: u16) -> Option<&UnitKind> {
        self.units.get(usize::from(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yeoman() -> UnitKind {
        UnitKind {
            id: "yeoman".into(),
            name: "Yeoman".into(),
            speed_tiles_per_s_x100: 180,
            radius_tiles_x100: 35,
            arrive_radius_tiles_x100: 25,
            arrive_slowdown_radius_tiles_x100: 50,
            waypoint_radius_tiles_x100: 50,
            separation_tiles_x100: 10,
        }
    }

    fn sample() -> Units {
        Units {
            units: vec![yeoman()],
        }
    }

    #[test]
    fn sample_is_valid() {
        sample().validate(Path::new("u.ron")).unwrap();
        assert_eq!(sample().kind(0).unwrap().id, "yeoman");
        assert!(sample().kind(1).is_none());
        assert!(sample().get("yeoman").is_some());
    }

    #[test]
    fn zero_speed_and_radius_are_rejected_with_field() {
        let mut u = sample();
        u.units[0].speed_tiles_per_s_x100 = 0;
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(
            err.contains("u.ron") && err.contains("units[0].speed_tiles_per_s_x100"),
            "{err}"
        );
        let mut u = sample();
        u.units[0].radius_tiles_x100 = 0;
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units[0].radius_tiles_x100"), "{err}");
        let mut u = sample();
        u.units[0].separation_tiles_x100 = MAX_X100 + 1;
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units[0].separation_tiles_x100"), "{err}");
        let mut u = sample();
        u.units[0].waypoint_radius_tiles_x100 = 0;
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units[0].waypoint_radius_tiles_x100"), "{err}");
        let mut u = sample();
        u.units[0].arrive_slowdown_radius_tiles_x100 = 24;
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(
            err.contains("units[0].arrive_slowdown_radius_tiles_x100"),
            "{err}"
        );
    }

    #[test]
    fn duplicate_and_empty_ids_are_rejected() {
        let mut u = sample();
        u.units.push(yeoman());
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units[1].id"), "{err}");
        let mut u = sample();
        u.units[0].id = "Yeoman".into();
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units[0].id"), "{err}");
        let u = Units::default();
        let err = u.validate(Path::new("u.ron")).unwrap_err().to_string();
        assert!(err.contains("units"), "{err}");
    }

    #[test]
    fn unknown_field_is_rejected_with_name() {
        let text = r#"(units: [(id: "yeoman", name: "Yeoman", speed_tiles_per_s_x100: 180, radius_tiles_x100: 35, arrive_radius_tiles_x100: 25, arrive_slowdown_radius_tiles_x100: 50, waypoint_radius_tiles_x100: 50, separation_tiles_x100: 10, hp: 3)])"#;
        let err = crate::parse_ron::<Units>(Path::new("u.ron"), text).unwrap_err();
        assert!(err.to_string().contains("hp"), "{err}");
    }
}
