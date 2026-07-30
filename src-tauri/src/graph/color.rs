// Copyright (C) 2026 Tom Cahill
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Color assignment and stability.
//!
//! Colors are keyed by stable branch identity, not raw lane index, so they don't visibly
//! reshuffle as pages load or branches move. `with_cache` seeds this stability from the
//! on-disk cache (`./color_cache.rs`) so it also survives an app restart, not just one
//! session.

use std::collections::HashMap;

/// What a lane's color is keyed on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BranchKey {
    Local(String),
    Remote(String),
    /// No ref at the lane's current tip (mid-history / not-yet-paginated-to-the-real-tip
    /// segment) — keyed on the oid that originally caused the lane to be allocated.
    Synthetic(String),
}

/// Number of palette slots reserved for persistent branches (`main`, `master`, etc.).
const PERSISTENT_SLOTS: u16 = 4;

pub struct ColorAssigner {
    assigned: HashMap<BranchKey, u16>,
    persistent_patterns: Vec<String>,
    next_persistent: u16,
    next_transient: u16,
}

impl ColorAssigner {
    /// Seeds `assigned` from a previously-persisted `{branch_name: color_id}` cache
    /// (`color_cache::load` — pass `&HashMap::new()` for no cache) so colors stay stable
    /// across app restarts, not just within one session. Only `Local` keys are ever cached —
    /// `Remote` names already carry their remote prefix (e.g. `origin/main`) so collisions
    /// with a `Local` name of the same string are rare in practice, and a `Synthetic` key is
    /// tied to one sweep's oid, not a stable branch identity, so it's never persisted at all
    /// (`ColorAssigner::named_colors`).
    ///
    /// `next_transient` is advanced past every cached transient color so a brand-new branch
    /// encountered this session can't be assigned a color a cached-but-not-yet-reencountered
    /// branch already owns.
    pub fn with_cache(
        persistent_patterns: Vec<String>,
        cached_colors: &HashMap<String, u16>,
    ) -> Self {
        let mut assigned = HashMap::new();
        let mut next_transient = PERSISTENT_SLOTS;
        for (name, &color_id) in cached_colors {
            assigned.insert(BranchKey::Local(name.clone()), color_id);
            if color_id >= PERSISTENT_SLOTS {
                next_transient = next_transient.max(color_id + 1);
            }
        }

        Self {
            assigned,
            persistent_patterns,
            next_persistent: 0,
            next_transient,
        }
    }

    /// Default persistent-branch patterns: `main`,
    /// `master`, `develop`, `trunk`. The repo's actual default branch (`origin/HEAD`) is
    /// added by the caller, since that requires a repository lookup this module doesn't do.
    pub fn default_persistent_patterns() -> Vec<String> {
        vec![
            "main".to_string(),
            "master".to_string(),
            "develop".to_string(),
            "trunk".to_string(),
        ]
    }

    fn is_persistent(&self, key: &BranchKey) -> bool {
        match key {
            BranchKey::Local(name) => self.persistent_patterns.iter().any(|p| p == name),
            _ => false,
        }
    }

    /// Returns the stable `color_id` for `key`, assigning one on first sight. Calling this
    /// again with an equal `key` always returns the same color for the lifetime of this
    /// `ColorAssigner`.
    pub fn color_for(&mut self, key: BranchKey) -> u16 {
        if let Some(&existing) = self.assigned.get(&key) {
            return existing;
        }

        let color_id = if self.is_persistent(&key) {
            let c = self.next_persistent;
            self.next_persistent = self.next_persistent.wrapping_add(1) % PERSISTENT_SLOTS;
            c
        } else {
            let c = self.next_transient;
            self.next_transient += 1;
            c
        };

        self.assigned.insert(key, color_id);
        color_id
    }

    /// Named (`Local`/`Remote`) branch colors currently assigned — what actually gets
    /// written back to the on-disk cache (`color_cache::save`). `Synthetic` entries are
    /// excluded: they're keyed on the oid that happened to allocate a lane in *this* sweep,
    /// not a stable branch identity, so persisting them would just cache noise.
    pub fn named_colors(&self) -> HashMap<String, u16> {
        self.assigned
            .iter()
            .filter_map(|(key, &color_id)| match key {
                BranchKey::Local(name) | BranchKey::Remote(name) => Some((name.clone(), color_id)),
                BranchKey::Synthetic(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_always_returns_the_same_color() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );
        let key = BranchKey::Local("feature/x".to_string());

        let first = colors.color_for(key.clone());
        let second = colors.color_for(key);

        assert_eq!(first, second);
    }

    #[test]
    fn distinct_keys_get_distinct_colors() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );

        let a = colors.color_for(BranchKey::Local("feature/a".to_string()));
        let b = colors.color_for(BranchKey::Local("feature/b".to_string()));

        assert_ne!(a, b);
    }

    #[test]
    fn persistent_branches_draw_from_the_reserved_slice() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );

        let main = colors.color_for(BranchKey::Local("main".to_string()));
        let develop = colors.color_for(BranchKey::Local("develop".to_string()));

        assert!(main < PERSISTENT_SLOTS);
        assert!(develop < PERSISTENT_SLOTS);
        assert_ne!(
            main, develop,
            "distinct persistent branches must draw distinct slots, not all collapse to the same one"
        );
    }

    #[test]
    fn persistent_slot_assignment_wraps_around_past_the_reserved_slice_size() {
        // More persistent-pattern branches than `PERSISTENT_SLOTS` (4) reserved slots, to
        // exercise `next_persistent`'s wraparound rather than just its first few assignments.
        let patterns = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut colors = ColorAssigner::with_cache(patterns, &HashMap::new());

        let assigned: Vec<u16> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|name| colors.color_for(BranchKey::Local(name.to_string())))
            .collect();

        assert_eq!(assigned, vec![0, 1, 2, 3, 0]);
    }

    #[test]
    fn transient_branches_never_collide_with_the_reserved_slice() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );

        for i in 0..10 {
            let c = colors.color_for(BranchKey::Local(format!("feature/{i}")));
            assert!(c >= PERSISTENT_SLOTS);
        }
    }

    #[test]
    fn synthetic_keys_are_distinguished_from_named_branches() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );

        let synthetic = colors.color_for(BranchKey::Synthetic("abc123".to_string()));
        let local = colors.color_for(BranchKey::Local("abc123".to_string()));

        assert_ne!(synthetic, local);
    }

    #[test]
    fn with_cache_reuses_a_previously_assigned_color_for_a_cached_branch() {
        let mut cached = HashMap::new();
        cached.insert("feature/x".to_string(), 7u16);
        let mut colors =
            ColorAssigner::with_cache(ColorAssigner::default_persistent_patterns(), &cached);

        let color = colors.color_for(BranchKey::Local("feature/x".to_string()));

        assert_eq!(color, 7);
    }

    #[test]
    fn with_cache_never_reassigns_a_cached_transient_color_to_a_new_branch() {
        let mut cached = HashMap::new();
        cached.insert("feature/x".to_string(), PERSISTENT_SLOTS); // first transient slot
        let mut colors =
            ColorAssigner::with_cache(ColorAssigner::default_persistent_patterns(), &cached);

        // A brand-new branch, never in the cache, must not collide with the cached color
        // above even though it would have been the very next transient slot assigned.
        let new_branch = colors.color_for(BranchKey::Local("feature/y".to_string()));

        assert_ne!(new_branch, PERSISTENT_SLOTS);
    }

    #[test]
    fn named_colors_excludes_synthetic_keys_but_includes_local_and_remote() {
        let mut colors = ColorAssigner::with_cache(
            ColorAssigner::default_persistent_patterns(),
            &HashMap::new(),
        );
        colors.color_for(BranchKey::Local("main".to_string()));
        colors.color_for(BranchKey::Remote("origin/main".to_string()));
        colors.color_for(BranchKey::Synthetic("abc123".to_string()));

        let named = colors.named_colors();

        assert!(named.contains_key("main"));
        assert!(named.contains_key("origin/main"));
        assert!(!named.contains_key("abc123"));
        assert_eq!(named.len(), 2);
    }
}
