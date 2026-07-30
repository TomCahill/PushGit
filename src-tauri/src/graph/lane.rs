// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lane assignment: a single top-to-bottom sweep over topologically-sorted commit rows.

use git2::Oid;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RailKind {
    /// Lane not touched by this row's commit; line continues unchanged.
    PassThrough,
    /// Edge to `parents[0]` — branch continuation or termination.
    ParentEdge,
    /// Edge to `parents[1..]` — a merge relationship.
    MergeEdge,
}

/// One lane's line segment as it passes through a row. `from_lane == to_lane` is a straight
/// line; different values mean a branch-out or merge-in curve.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rail {
    pub from_lane: u16,
    pub to_lane: u16,
    pub color_id: u16,
    pub kind: RailKind,
}

struct LaneState {
    awaited_oid: Oid,
    color_id: u16,
}

pub struct RowLayout {
    pub lane: u16,
    pub color_id: u16,
    pub rails: Vec<Rail>,
}

/// Tracks which lane (column) each active branch occupies, one sweep step at a time.
/// Lane indices are recycled via a free-list (lowest index wins on allocation).
#[derive(Default)]
pub struct LaneTracker {
    lanes: Vec<Option<LaneState>>,
}

impl LaneTracker {
    pub fn new() -> Self {
        Self { lanes: Vec::new() }
    }

    /// The number of lanes currently in use — the high-water mark relevant to
    /// high-fan-out handling. Not yet surfaced outside
    /// tests, where it verifies the free-list correctly releases converged/terminated
    /// lanes; wire this into `GraphSession` once the lane-count threshold UI exists.
    #[allow(dead_code)]
    pub fn active_lane_count(&self) -> usize {
        self.lanes.iter().filter(|l| l.is_some()).count()
    }

    /// Processes one commit row. `color_for` is called with the oid of whichever commit is
    /// starting a *new* lane (the current commit itself, if nothing was waiting for it, or a
    /// merge parent that no active lane already awaits) so the caller can resolve a stable
    /// `BranchKey` for it.
    pub fn process(
        &mut self,
        oid: Oid,
        parents: &[Oid],
        mut color_for: impl FnMut(Oid) -> u16,
    ) -> RowLayout {
        let waiting: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.as_ref().filter(|s| s.awaited_oid == oid).map(|_| i))
            .collect();

        let own_lane = waiting
            .iter()
            .copied()
            .min()
            .unwrap_or_else(|| self.allocate_free_lane());

        let color_id = if let Some(state) = &self.lanes[own_lane] {
            state.color_id
        } else {
            color_for(oid)
        };

        let mut rails = Vec::new();

        // Free every other lane that was also waiting for this oid — two branches converging
        // on the same ancestor (common above merge bases).
        for &lane in &waiting {
            if lane != own_lane {
                let other_color = self.lanes[lane].as_ref().unwrap().color_id;
                rails.push(Rail {
                    from_lane: lane as u16,
                    to_lane: own_lane as u16,
                    color_id: other_color,
                    kind: RailKind::PassThrough,
                });
                self.lanes[lane] = None;
            }
        }

        // Advance the chosen lane to the first parent, or terminate it at a root commit.
        match parents.first() {
            Some(&first_parent) => {
                self.lanes[own_lane] = Some(LaneState {
                    awaited_oid: first_parent,
                    color_id,
                });
                rails.push(Rail {
                    from_lane: own_lane as u16,
                    to_lane: own_lane as u16,
                    color_id,
                    kind: RailKind::ParentEdge,
                });
            }
            None => self.lanes[own_lane] = None,
        }

        // Route additional parents (merges): reuse an already-waiting lane if one exists,
        // otherwise allocate a new one.
        for &parent in parents.iter().skip(1) {
            let existing = self
                .lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|s| s.awaited_oid == parent));

            let (target_lane, target_color) = match existing {
                Some(lane) => (lane, self.lanes[lane].as_ref().unwrap().color_id),
                None => {
                    let new_lane = self.allocate_free_lane();
                    let new_color = color_for(parent);
                    self.lanes[new_lane] = Some(LaneState {
                        awaited_oid: parent,
                        color_id: new_color,
                    });
                    (new_lane, new_color)
                }
            };

            rails.push(Rail {
                from_lane: own_lane as u16,
                to_lane: target_lane as u16,
                color_id: target_color,
                kind: RailKind::MergeEdge,
            });
        }

        // Every other still-active lane simply continues straight down through this row.
        for (i, lane) in self.lanes.iter().enumerate() {
            if i == own_lane || waiting.contains(&i) {
                continue;
            }
            if let Some(state) = lane {
                rails.push(Rail {
                    from_lane: i as u16,
                    to_lane: i as u16,
                    color_id: state.color_id,
                    kind: RailKind::PassThrough,
                });
            }
        }

        RowLayout {
            lane: own_lane as u16,
            color_id,
            rails,
        }
    }

    fn allocate_free_lane(&mut self) -> usize {
        match self.lanes.iter().position(|l| l.is_none()) {
            Some(i) => i,
            None => {
                self.lanes.push(None);
                self.lanes.len() - 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> Oid {
        let mut bytes = [0u8; 20];
        bytes[19] = byte;
        Oid::from_bytes(&bytes).unwrap()
    }

    fn next_color(counter: &mut u16) -> impl FnMut(Oid) -> u16 + '_ {
        move |_| {
            let c = *counter;
            *counter += 1;
            c
        }
    }

    #[test]
    fn linear_history_stays_on_a_single_lane() {
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        let c1 = tracker.process(oid(3), &[oid(2)], next_color(&mut colors));
        let c2 = tracker.process(oid(2), &[oid(1)], next_color(&mut colors));
        let c3 = tracker.process(oid(1), &[], next_color(&mut colors));

        assert_eq!(c1.lane, 0);
        assert_eq!(c2.lane, 0);
        assert_eq!(c3.lane, 0);
        assert_eq!(c1.color_id, c2.color_id);
        assert_eq!(c2.color_id, c3.color_id);
    }

    #[test]
    fn a_root_commit_with_no_parents_terminates_its_lane() {
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        tracker.process(oid(1), &[], next_color(&mut colors));

        assert_eq!(tracker.active_lane_count(), 0);
    }

    #[test]
    fn diverging_branches_get_distinct_lanes() {
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        // A merge commit `m` with two parents `a` and `b` that never converge in this
        // fragment: `b` should get its own lane distinct from `m`'s.
        let m = tracker.process(oid(10), &[oid(1), oid(2)], next_color(&mut colors));
        assert_eq!(tracker.active_lane_count(), 2);

        let a = tracker.process(oid(1), &[], next_color(&mut colors));
        let b = tracker.process(oid(2), &[], next_color(&mut colors));

        assert_ne!(a.lane, b.lane);
        assert_eq!(
            m.rails
                .iter()
                .filter(|r| r.kind == RailKind::MergeEdge)
                .count(),
            1
        );
    }

    #[test]
    fn converging_branches_free_the_extra_lane() {
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        // Two independent tips (`a`, `b`) both awaiting the same ancestor `base`.
        tracker.process(oid(1), &[oid(3)], next_color(&mut colors));
        tracker.process(oid(2), &[oid(3)], next_color(&mut colors));
        assert_eq!(tracker.active_lane_count(), 2);

        let base = tracker.process(oid(3), &[], next_color(&mut colors));

        // One lane converges into the other; only one lane should remain (then terminate).
        assert_eq!(tracker.active_lane_count(), 0);
        assert!(base
            .rails
            .iter()
            .any(|r| r.kind == RailKind::PassThrough && r.to_lane == base.lane));
    }

    #[test]
    fn a_fresh_lane_s_first_row_produces_no_spurious_pass_through_rail() {
        // The very first commit processed by a brand-new tracker allocates its lane fresh
        // (nothing was `waiting` for it) — own_lane's own just-updated state must still be
        // excluded from the "every other lane passes through" sweep, or it'd get both its
        // real `ParentEdge` rail *and* a spurious extra `PassThrough` for the same lane.
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        let row = tracker.process(oid(2), &[oid(1)], next_color(&mut colors));

        assert_eq!(row.rails.len(), 1);
        assert_eq!(row.rails[0].kind, RailKind::ParentEdge);
    }

    #[test]
    fn unrelated_lanes_pass_through_untouched() {
        let mut tracker = LaneTracker::new();
        let mut colors = 0u16;

        tracker.process(oid(1), &[oid(2)], next_color(&mut colors));
        let unrelated = tracker.process(oid(9), &[oid(8)], next_color(&mut colors));

        // Processing an unrelated root commit should emit a pass-through rail for lane 0
        // (still awaiting oid(2)) without disturbing its state.
        assert!(unrelated
            .rails
            .iter()
            .any(|r| r.kind == RailKind::PassThrough && r.from_lane == 0 && r.to_lane == 0));
    }

    proptest::proptest! {
        /// Generalizes `linear_history_stays_on_a_single_lane` across arbitrary chain
        /// lengths — a branch's lane must persist unchanged as long as it's walking its
        /// own first-parent chain.
        #[test]
        fn linear_chains_of_any_length_stay_on_lane_zero(length in 1usize..100) {
            let mut tracker = LaneTracker::new();
            let mut colors = 0u16;
            let oids: Vec<Oid> = (0..length).map(|i| oid(i as u8)).collect();

            let mut lanes_seen = Vec::with_capacity(length);
            let mut colors_seen = Vec::with_capacity(length);
            for i in 0..length {
                let parents: Vec<Oid> = if i + 1 < length { vec![oids[i + 1]] } else { vec![] };
                let layout = tracker.process(oids[i], &parents, next_color(&mut colors));
                lanes_seen.push(layout.lane);
                colors_seen.push(layout.color_id);
            }

            proptest::prop_assert!(lanes_seen.iter().all(|&l| l == 0));
            proptest::prop_assert!(colors_seen.iter().all(|&c| c == colors_seen[0]));
            proptest::prop_assert_eq!(tracker.active_lane_count(), 0);
        }

        /// Processing the same generated history through two fresh trackers must produce
        /// identical lane/color assignments — a regression guard against accidentally
        /// introducing iteration-order-dependent (e.g. hash-map-based) non-determinism.
        #[test]
        fn processing_is_deterministic(length in 1usize..50, merge_every in 2usize..10) {
            let oids: Vec<Oid> = (0..length).map(|i| oid(i as u8)).collect();
            let rows: Vec<(Oid, Vec<Oid>)> = (0..length)
                .map(|i| {
                    let mut parents = Vec::new();
                    if i + 1 < length {
                        parents.push(oids[i + 1]);
                    }
                    if merge_every > 0 && i % merge_every == 0 && i + 2 < length {
                        parents.push(oids[i + 2]);
                    }
                    (oids[i], parents)
                })
                .collect();

            let run = |rows: &[(Oid, Vec<Oid>)]| {
                let mut tracker = LaneTracker::new();
                let mut colors = 0u16;
                rows.iter()
                    .map(|(oid, parents)| {
                        let layout = tracker.process(*oid, parents, next_color(&mut colors));
                        (layout.lane, layout.color_id)
                    })
                    .collect::<Vec<_>>()
            };

            proptest::prop_assert_eq!(run(&rows), run(&rows));
        }
    }
}
