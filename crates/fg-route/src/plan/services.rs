//! Expected-kill service visits. A due maximum commits to the next trip.
use super::state::{Event, State};
use super::{Planner, ServiceKind};
use crate::world::Pos;

impl Planner<'_> {
    pub(crate) fn service_settings(&self, kind: ServiceKind) -> (bool, f64, f64, f64) {
        match kind {
            ServiceKind::Vendor => (
                self.params.vendor_visits,
                self.params.vendor_min_kills,
                self.params.vendor_max_kills,
                self.params.vendor_visit_seconds,
            ),
            ServiceKind::Mailbox => (
                self.params.mailbox_visits,
                self.params.mailbox_min_kills,
                self.params.mailbox_max_kills,
                self.params.mailbox_visit_seconds,
            ),
        }
    }

    /// No recursion: finish each selected visit before considering the other service.
    pub(crate) fn service_visits(
        &self,
        s: &mut State,
        target: Option<Pos>,
        force: bool,
        opportunistic: bool,
        record: &mut impl FnMut(&State, Event),
    ) {
        if self.explicit || !s.time.is_finite() {
            return;
        }
        for kind in [ServiceKind::Vendor, ServiceKind::Mailbox] {
            let (enabled, min, max, duration) = self.service_settings(kind);
            if !enabled {
                continue;
            }
            let kills = s.service_kills[kind.index()];
            let mandatory = force || kills >= max;
            if !mandatory && (!opportunistic || kills < min) {
                continue;
            }
            let mut choices: Vec<_> = self.model.services[kind.index()]
                .iter()
                .enumerate()
                .filter_map(|(i, loc)| {
                    let d = s.pos.dist(&loc.pos);
                    d.is_finite().then_some((i, d))
                })
                .collect();
            choices.sort_unstable_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let direct = target.map_or(0.0, |p| self.trips().fastest(s, &p).0);
            let mut best: Option<(usize, f64)> = None;
            for (i, _) in choices.into_iter().take(12) {
                let loc = &self.model.services[kind.index()][i];
                let there = self.trips().fastest(s, &loc.pos).0;
                let cost = if mandatory {
                    there
                } else {
                    // A conservative extra-walking estimate; the complete detour is replayed.
                    there + target.map_or(0.0, |p| self.trips().walk(&loc.pos, &p, s.level)) - direct + duration
                };
                if cost.is_finite() && best.is_none_or(|(_, old)| cost < old) {
                    best = Some((i, cost));
                }
            }
            match best {
                Some((i, cost)) if mandatory || cost <= 120.0 => self.service_at(s, kind, i, record),
                None if mandatory => {
                    s.time = f64::INFINITY;
                    return;
                }
                _ => {}
            }
        }
    }

    pub(crate) fn service_at(
        &self,
        s: &mut State,
        kind: ServiceKind,
        index: usize,
        record: &mut impl FnMut(&State, Event),
    ) {
        let loc = &self.model.services[kind.index()][index];
        self.go(s, &loc.pos, loc.zone, record);
        if !s.time.is_finite() {
            return;
        }
        let duration = self.service_settings(kind).3;
        s.time += duration;
        s.spent.overhead += duration;
        s.service_kills[kind.index()] = 0.0;
        record(s, Event::Service { kind, index });
    }
}
