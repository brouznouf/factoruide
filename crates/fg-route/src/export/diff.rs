//! Differences between two guides (two versions of one), for the app's comparison and as text
//! (`fg diff`, to hand to someone looking for what went wrong).
//!
//! The order of the steps matters: steps are aligned in order (a longest common subsequence),
//! so that a step added or removed is one line, not a shift of everything after it. Steps are
//! grouped in blocks (consecutive steps in one zone, a dungeon run): blocks are aligned first,
//! then the steps of each pair of blocks. A block found at another place (two zones done the
//! other way round) is one move, with the differences of its steps; a step found at another place
//! (another zone, further in the same one) is a move too. The time and level of the steps follow
//! from everything before them and are not differences.

use super::types::{Route, Step, StepKind};
use crate::plan::{Breakdown, fmt_time};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;

/// A step farther than this from its place in the other guide (percent of the map) has moved.
const MOVED_SPOT: f64 = 2.0;
/// Two blocks of one zone not aligned in order are one moved when they share this much of the
/// smaller one's steps.
const MOVED_BLOCK_SHARE: f64 = 0.5;
/// Identical steps shown around a difference in the text.
const CONTEXT: usize = 1;

#[derive(Debug, Clone, Serialize)]
pub struct RouteDiff {
    pub summary: Summary,
    pub blocks: Vec<BlockDiff>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    /// Steps added, removed (not moved), changed in place, and moved (each move counted once).
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub moved: usize,
    /// Blocks moved as a whole, added, removed.
    pub blocks_moved: usize,
    pub blocks_added: usize,
    pub blocks_removed: usize,
}

impl Summary {
    pub fn is_empty(&self) -> bool {
        self.added + self.removed + self.changed + self.moved + self.blocks_moved == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockStatus {
    Same,
    Changed,
    Added,
    Removed,
    /// Done at another place of the guide: here at its place in the second guide, with the
    /// differences of its steps.
    Moved,
    /// Where a moved block was in the first guide (`b`: where it went, no rows).
    MovedAway,
}

/// First and last steps (indices) of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub first: usize,
    pub last: usize,
}

impl Span {
    fn len(self) -> usize {
        self.last - self.first + 1
    }
}

/// A block of the comparison, in the order of the second guide (blocks of the first one only
/// where they were).
#[derive(Debug, Clone, Serialize)]
pub struct BlockDiff {
    pub status: BlockStatus,
    /// Zone (or dungeon) of the block, as the second guide names it when it has the block.
    pub zone: Option<String>,
    pub dungeon: bool,
    pub a: Option<Span>,
    pub b: Option<Span>,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Same,
    Changed,
    Removed,
    Added,
}

/// A step of one guide or both (indices in their steps).
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub op: Op,
    pub a: Option<usize>,
    pub b: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
    /// A removed step done elsewhere in the second guide (its index there), or an added one done
    /// elsewhere in the first (its index there; `changes`: what differs from it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved: Option<usize>,
}

/// What differs between the two sides of a step (or of a setting).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub field: String,
    pub from: String,
    pub to: String,
}

/// What makes a step the same step in both guides; what else differs is a change.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    kind: StepKind,
    quest: Option<i64>,
    objective: Option<u8>,
    target: Option<i64>,
}

impl Key {
    fn of(s: &Step) -> Self {
        // Where to fly, which transport or dungeon is what the step is; which trainer, inn or
        // grinding goal is a change of the step.
        let target = match s.kind {
            StepKind::Fly | StepKind::FlightMaster | StepKind::Link | StepKind::Dungeon => {
                s.target.as_ref().map(|t| t.id)
            }
            StepKind::Profession | StepKind::Practice => s.line,
            _ => None,
        };
        Self {
            kind: s.kind,
            quest: s.quest,
            objective: s.objective,
            target,
        }
    }

    /// Steps that are one particular thing (a quest, a place) can be found elsewhere: a
    /// hearthstone or a grind elsewhere is another one.
    fn movable(&self) -> bool {
        self.quest.is_some() || self.target.is_some()
    }
}

/// Consecutive steps in one zone, or a dungeon run with the quests done inside.
struct Block {
    /// Dungeon or zone: the dungeon's ID, the zone's English name (the guide's `zone` before it
    /// is written in a language; guides of format 1 have no zone IDs).
    key: Option<(bool, String)>,
    zone: Option<String>,
    dungeon: bool,
    span: Span,
}

fn blocks(steps: &[Step]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        if let Some(last) = out.last_mut()
            && (s.inside && last.dungeon
                || !last.dungeon && s.kind != StepKind::Dungeon && (s.zone.is_none() || s.zone == last.zone))
        {
            last.span.last = i;
            continue;
        }
        let dungeon = s.kind == StepKind::Dungeon;
        let (key, zone) = if dungeon {
            (
                s.target.as_ref().map(|t| (true, t.id.to_string())),
                s.target.as_ref().map(|t| t.name.clone()).or_else(|| s.zone.clone()),
            )
        } else {
            (s.zone.clone().map(|z| (false, z)), s.zone.clone())
        };
        out.push(Block {
            key,
            zone,
            dungeon,
            span: Span { first: i, last: i },
        });
    }
    out
}

/// The differences between guide `a` and guide `b`, as saved (not written in another language:
/// zones are told apart by their English names).
pub fn diff(a: &Route, b: &Route) -> RouteDiff {
    let (ka, kb): (Vec<Key>, Vec<Key>) = (
        a.steps.iter().map(Key::of).collect(),
        b.steps.iter().map(Key::of).collect(),
    );
    let (ba, bb) = (blocks(&a.steps), blocks(&b.steps));
    let (ca, cb): (Vec<_>, Vec<_>) = (
        ba.iter().map(|x| counts(&ka, x)).collect(),
        bb.iter().map(|x| counts(&kb, x)).collect(),
    );
    let common = |i: usize, j: usize| -> usize {
        ca[i]
            .iter()
            .map(|(k, n)| (*n).min(cb[j].get(k).copied().unwrap_or(0)))
            .sum()
    };

    // Blocks in order: the pairs of one zone sharing the most steps (a zone met with nothing in
    // common still pairs, half as much as a step in common).
    let anchors = align(ba.len(), bb.len(), |i, j| {
        (ba[i].key == bb[j].key).then(|| 2 * common(i, j) + 1)
    });
    let mut pair_a: Vec<Option<usize>> = vec![None; ba.len()];
    let mut pair_b: Vec<Option<usize>> = vec![None; bb.len()];
    for &(i, j) in &anchors {
        pair_a[i] = Some(j);
        pair_b[j] = Some(i);
    }
    // Blocks out of order: moved when most of their steps are the same.
    let mut moves: Vec<(usize, usize, usize)> = Vec::new();
    for (i, x) in ba.iter().enumerate().filter(|(i, _)| pair_a[*i].is_none()) {
        for (j, y) in bb.iter().enumerate().filter(|(j, _)| pair_b[*j].is_none()) {
            let n = common(i, j);
            if x.key == y.key && n > 0 && n as f64 >= MOVED_BLOCK_SHARE * x.span.len().min(y.span.len()) as f64 {
                moves.push((n, i, j));
            }
        }
    }
    moves.sort_by_key(|&(n, i, j)| (std::cmp::Reverse(n), i, j));
    let mut moved_a: Vec<Option<usize>> = vec![None; ba.len()];
    let mut moved_b: Vec<Option<usize>> = vec![None; bb.len()];
    for (_, i, j) in moves {
        if moved_a[i].is_none() && moved_b[j].is_none() {
            moved_a[i] = Some(j);
            moved_b[j] = Some(i);
        }
    }

    // In the order of `b`, the blocks of `a` alone where they were.
    let rows_of = |i: usize, j: usize| step_rows(a, b, &ka, &kb, ba[i].span, bb[j].span);
    let mut out: Vec<BlockDiff> = Vec::new();
    let only_a = |i: usize, out: &mut Vec<BlockDiff>| {
        let x = &ba[i];
        out.push(match moved_a[i] {
            Some(j) => BlockDiff {
                status: BlockStatus::MovedAway,
                zone: bb[j].zone.clone(),
                dungeon: x.dungeon,
                a: Some(x.span),
                b: Some(bb[j].span),
                rows: Vec::new(),
            },
            None => BlockDiff {
                status: BlockStatus::Removed,
                zone: x.zone.clone(),
                dungeon: x.dungeon,
                a: Some(x.span),
                b: None,
                rows: (x.span.first..=x.span.last)
                    .map(|k| row(Op::Removed, Some(k), None))
                    .collect(),
            },
        });
    };
    let only_b = |j: usize, out: &mut Vec<BlockDiff>| {
        let y = &bb[j];
        out.push(match moved_b[j] {
            Some(i) => BlockDiff {
                status: BlockStatus::Moved,
                zone: y.zone.clone(),
                dungeon: y.dungeon,
                a: Some(ba[i].span),
                b: Some(y.span),
                rows: rows_of(i, j),
            },
            None => BlockDiff {
                status: BlockStatus::Added,
                zone: y.zone.clone(),
                dungeon: y.dungeon,
                a: None,
                b: Some(y.span),
                rows: (y.span.first..=y.span.last)
                    .map(|k| row(Op::Added, None, Some(k)))
                    .collect(),
            },
        });
    };
    let (mut pa, mut pb) = (0, 0);
    for &(i, j) in anchors.iter().chain(std::iter::once(&(ba.len(), bb.len()))) {
        (pa..i).for_each(|k| only_a(k, &mut out));
        (pb..j).for_each(|k| only_b(k, &mut out));
        if i < ba.len() {
            let rows = rows_of(i, j);
            let same = rows.iter().all(|r| r.op == Op::Same);
            out.push(BlockDiff {
                status: if same { BlockStatus::Same } else { BlockStatus::Changed },
                zone: bb[j].zone.clone(),
                dungeon: bb[j].dungeon,
                a: Some(ba[i].span),
                b: Some(bb[j].span),
                rows,
            });
        }
        (pa, pb) = (i + 1, j + 1);
    }

    find_moved_steps(&mut out, a, b, &ka, &kb);
    for blk in &mut out {
        if blk.status == BlockStatus::Same && blk.rows.iter().any(|r| r.op != Op::Same) {
            blk.status = BlockStatus::Changed;
        }
    }
    let summary = summarize(&out);
    RouteDiff { summary, blocks: out }
}

/// How many times each step is in a block.
fn counts<'a>(keys: &'a [Key], blk: &Block) -> HashMap<&'a Key, usize> {
    let mut m: HashMap<&Key, usize> = HashMap::new();
    for k in &keys[blk.span.first..=blk.span.last] {
        *m.entry(k).or_default() += 1;
    }
    m
}

fn row(op: Op, a: Option<usize>, b: Option<usize>) -> Row {
    Row {
        op,
        a,
        b,
        changes: Vec::new(),
        moved: None,
    }
}

/// The steps of two blocks aligned in order.
fn step_rows(a: &Route, b: &Route, ka: &[Key], kb: &[Key], sa: Span, sb: Span) -> Vec<Row> {
    let pairs = align(sa.len(), sb.len(), |i, j| {
        (ka[sa.first + i] == kb[sb.first + j]).then_some(1)
    });
    let mut rows = Vec::new();
    let (mut pa, mut pb) = (0, 0);
    for &(i, j) in pairs.iter().chain(std::iter::once(&(sa.len(), sb.len()))) {
        rows.extend((pa..i).map(|k| row(Op::Removed, Some(sa.first + k), None)));
        rows.extend((pb..j).map(|k| row(Op::Added, None, Some(sb.first + k))));
        if i < sa.len() {
            let (x, y) = (sa.first + i, sb.first + j);
            let changes = changes(&a.steps[x], &b.steps[y]);
            rows.push(Row {
                op: if changes.is_empty() { Op::Same } else { Op::Changed },
                changes,
                ..row(Op::Same, Some(x), Some(y))
            });
        }
        (pa, pb) = (i + 1, j + 1);
    }
    rows
}

/// A step removed in one place and added in another is one step moved (its differences on the
/// added side).
fn find_moved_steps(blocks: &mut [BlockDiff], a: &Route, b: &Route, ka: &[Key], kb: &[Key]) {
    let mut added: HashMap<&Key, Vec<(usize, usize)>> = HashMap::new();
    for (bi, blk) in blocks.iter().enumerate() {
        for (ri, r) in blk.rows.iter().enumerate() {
            if let (Op::Added, Some(j)) = (r.op, r.b)
                && kb[j].movable()
            {
                added.entry(&kb[j]).or_default().push((bi, ri));
            }
        }
    }
    let mut pairs = Vec::new();
    for (bi, blk) in blocks.iter().enumerate() {
        for (ri, r) in blk.rows.iter().enumerate() {
            if let (Op::Removed, Some(i)) = (r.op, r.a)
                && let Some(list) = added.get_mut(&ka[i])
                && !list.is_empty()
            {
                pairs.push(((bi, ri), list.remove(0)));
            }
        }
    }
    for ((bi, ri), (bj, rj)) in pairs {
        let (Some(i), Some(j)) = (blocks[bi].rows[ri].a, blocks[bj].rows[rj].b) else {
            continue;
        };
        blocks[bi].rows[ri].moved = Some(j);
        blocks[bj].rows[rj].moved = Some(i);
        blocks[bj].rows[rj].changes = changes(&a.steps[i], &b.steps[j]);
    }
}

fn summarize(blocks: &[BlockDiff]) -> Summary {
    let mut s = Summary::default();
    for blk in blocks {
        match blk.status {
            BlockStatus::Moved => s.blocks_moved += 1,
            BlockStatus::Added => s.blocks_added += 1,
            BlockStatus::Removed => s.blocks_removed += 1,
            _ => {}
        }
        for r in &blk.rows {
            match (r.op, r.moved) {
                (Op::Changed, _) => s.changed += 1,
                (Op::Added, Some(_)) => s.moved += 1,
                (Op::Added, None) => s.added += 1,
                (Op::Removed, None) => s.removed += 1,
                _ => {}
            }
        }
    }
    s
}

/// The pairs `(i, j)`, increasing on both sides, of the most total weight (`weight`: none when
/// `i` and `j` cannot pair).
fn align(n: usize, m: usize, weight: impl Fn(usize, usize) -> Option<usize>) -> Vec<(usize, usize)> {
    let w = m + 1;
    let mut best = vec![0; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let skip = best[(i + 1) * w + j].max(best[i * w + j + 1]);
            let take = weight(i, j).map_or(0, |x| x + best[(i + 1) * w + j + 1]);
            best[i * w + j] = skip.max(take);
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if let Some(x) = weight(i, j)
            && best[i * w + j] == x + best[(i + 1) * w + j + 1]
        {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if best[i * w + j] == best[(i + 1) * w + j] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// What differs between two sides of one step, but what follows from the rest of the guide (its
/// time, level, mobs farmed on the way to it).
fn changes(a: &Step, b: &Step) -> Vec<Change> {
    let mut out = Vec::new();
    let mut push = |field: &str, from: String, to: String| {
        out.push(Change {
            field: field.to_owned(),
            from,
            to,
        });
    };
    match a.kind {
        // Their sentences say how long they take, which follows from the rest of the guide.
        StepKind::Grind => {
            if (a.level, a.xp) != (b.level, b.xp) {
                push("goal", grind_goal(a), grind_goal(b));
            }
        }
        StepKind::Dungeon => {}
        _ => {
            // The sentences before they are written in a language, when the guide has them.
            let same = if a.say.is_empty() || b.say.is_empty() {
                a.text == b.text
            } else {
                a.say == b.say
            };
            if !same {
                push("text", a.text.clone(), b.text.clone());
            } else if a.target.as_ref().map(|t| (t.kind, t.id)) != b.target.as_ref().map(|t| (t.kind, t.id)) {
                // Another innkeeper of the same town.
                push("target", target_name(a), target_name(b));
            }
        }
    }
    let spot = |s: &Step| s.map.as_ref().map(|m| (m.ui_map, m.x, m.y));
    if let (Some(x), Some(y)) = (spot(a), spot(b))
        && (x.0 != y.0 || (x.1 - y.1).hypot(x.2 - y.2) > MOVED_SPOT)
    {
        push("position", spot_text(a), spot_text(b));
    }
    if a.reward != b.reward {
        push("reward", opt(a.reward), opt(b.reward));
    }
    if a.bg != b.bg {
        push("along the way", a.bg.to_string(), b.bg.to_string());
    }
    out
}

fn grind_goal(s: &Step) -> String {
    match s.xp {
        Some(xp) => format!("level {} + {xp} xp", s.level),
        None => format!("level {}", s.level),
    }
}

fn target_name(s: &Step) -> String {
    s.target.as_ref().map_or_else(|| "-".to_owned(), |t| t.name.clone())
}

fn spot_text(s: &Step) -> String {
    s.map.as_ref().map_or_else(
        || "-".to_owned(),
        |m| format!("{:.1}, {:.1} (map {})", m.x, m.y, m.ui_map),
    )
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "-".to_owned(), |x| x.to_string())
}

/// The settings that differ between two JSON values (dotted paths; lists compared whole).
pub fn value_changes(a: &Value, b: &Value) -> Vec<Change> {
    fn walk(path: &str, a: Option<&Value>, b: Option<&Value>, out: &mut Vec<Change>) {
        if let (Some(Value::Object(x)), Some(Value::Object(y))) = (a, b) {
            let mut keys: Vec<&String> = x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))).collect();
            keys.sort();
            for k in keys {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                walk(&p, x.get(k), y.get(k), out);
            }
        } else if a.unwrap_or(&Value::Null) != b.unwrap_or(&Value::Null) {
            out.push(Change {
                field: path.to_owned(),
                from: short(a),
                to: short(b),
            });
        }
    }
    let mut out = Vec::new();
    walk("", Some(a), Some(b), &mut out);
    out
}

fn short(v: Option<&Value>) -> String {
    const MAX: usize = 80;
    let s = match v {
        None | Some(Value::Null) => "-".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    if s.chars().count() > MAX {
        format!("{}…", s.chars().take(MAX).collect::<String>())
    } else {
        s
    }
}

/// The comparison as text: the figures of both guides, the settings changed, then each block
/// with a difference and its steps (`-` removed, `+` added, `~` changed, `<` and `>` moved).
/// `names`: what to call each guide (v3, a file name).
pub fn to_text(diff: &RouteDiff, a: &Route, b: &Route, names: [&str; 2], settings: &[Change]) -> String {
    let [na, nb] = names;
    let mut t = String::new();
    let _ = writeln!(t, "Guide {}: {na} -> {nb}", b.name);
    let _ = writeln!(
        t,
        "Time {} -> {} ({}), quests {} -> {}, steps {} -> {}, levels {}-{} -> {}-{}",
        fmt_time(a.total_time),
        fmt_time(b.total_time),
        signed_time(b.total_time - a.total_time),
        a.quests,
        b.quests,
        a.steps.len(),
        b.steps.len(),
        a.from_level,
        a.to_level,
        b.from_level,
        b.to_level,
    );
    let parts = breakdown_changes(&a.breakdown, &b.breakdown);
    if !parts.is_empty() {
        let _ = writeln!(t, "Breakdown: {}", parts.join(", "));
    }
    for c in settings {
        let _ = writeln!(t, "Setting {}: {} -> {}", c.field, c.from, c.to);
    }
    let s = &diff.summary;
    if s.is_empty() {
        let _ = writeln!(t, "Same steps.");
        return t;
    }
    let _ = writeln!(
        t,
        "Steps: {} added, {} removed, {} changed, {} moved; zone blocks: {} moved, {} added, {} removed",
        s.added, s.removed, s.changed, s.moved, s.blocks_moved, s.blocks_added, s.blocks_removed
    );
    let n = |i: usize| i + 1;
    let span = |name: &str, sp: Option<Span>| sp.map(|x| format!("{name} #{}-{}", n(x.first), n(x.last)));
    let mut same_run: Vec<&BlockDiff> = Vec::new();
    let flush = |run: &mut Vec<&BlockDiff>, t: &mut String| {
        if let (Some(first), Some(last)) = (run.first(), run.last()) {
            let zones: Vec<&str> = run.iter().map(|x| zone_name(x)).collect();
            let _ = writeln!(
                t,
                "\n= {} identical: {na} #{}-{} | {nb} #{}-{} ({})",
                plural(run.len(), "block"),
                n(first.a.expect("a block present on both sides").first),
                n(last.a.expect("a block present on both sides").last),
                n(first.b.expect("a block present on both sides").first),
                n(last.b.expect("a block present on both sides").last),
                zones.join(", ")
            );
        }
        run.clear();
    };
    for blk in &diff.blocks {
        if blk.status == BlockStatus::Same {
            same_run.push(blk);
            continue;
        }
        flush(&mut same_run, &mut t);
        let sides = [span(na, blk.a), span(nb, blk.b)];
        let where_ = match blk.status {
            BlockStatus::Moved | BlockStatus::MovedAway => {
                sides.iter().flatten().cloned().collect::<Vec<_>>().join(" -> ")
            }
            _ => sides.iter().flatten().cloned().collect::<Vec<_>>().join(" | "),
        };
        let label = match blk.status {
            BlockStatus::Changed => "",
            BlockStatus::Added => "added: ",
            BlockStatus::Removed => "removed: ",
            BlockStatus::Moved => "moved here: ",
            BlockStatus::MovedAway => "moved away: ",
            BlockStatus::Same => unreachable!(),
        };
        let levels = blk
            .b
            .map(|x| (&b.steps, x))
            .or(blk.a.map(|x| (&a.steps, x)))
            .map(|(steps, x)| level_range(steps[x.first].level, steps[x.last].level))
            .unwrap_or_default();
        let _ = writeln!(t, "\n@@ {label}{} {where_} ({levels})", zone_name(blk));
        if blk.status == BlockStatus::MovedAway {
            continue;
        }
        write_rows(&mut t, &blk.rows, a, b, names);
    }
    flush(&mut same_run, &mut t);
    t
}

fn write_rows(t: &mut String, rows: &[Row], a: &Route, b: &Route, [na, nb]: [&str; 2]) {
    let step = |s: &Step| format!("L{} {}: {}", s.level, s.kind.as_str(), s.text);
    let shown: Vec<bool> = (0..rows.len())
        .map(|k| {
            let lo = k.saturating_sub(CONTEXT);
            let hi = (k + CONTEXT).min(rows.len() - 1);
            rows[lo..=hi].iter().any(|r| r.op != Op::Same)
        })
        .collect();
    let mut k = 0;
    while k < rows.len() {
        if !shown[k] {
            let start = k;
            while k < rows.len() && !shown[k] {
                k += 1;
            }
            let (x, y) = (&rows[start], &rows[k - 1]);
            let _ = writeln!(
                t,
                "  ... {} identical ({na} #{}-{} | {nb} #{}-{})",
                plural(k - start, "step"),
                x.a.expect("a step present on both sides") + 1,
                y.a.expect("a step present on both sides") + 1,
                x.b.expect("a step present on both sides") + 1,
                y.b.expect("a step present on both sides") + 1
            );
            continue;
        }
        let r = &rows[k];
        let _ = match (r.op, r.a, r.b, r.moved) {
            (Op::Same, Some(i), Some(j), _) => writeln!(t, "  {na}#{} {nb}#{} {}", i + 1, j + 1, step(&b.steps[j])),
            (Op::Changed, Some(i), Some(j), _) => {
                let _ = writeln!(t, "~ {na}#{} {nb}#{} {}", i + 1, j + 1, step(&b.steps[j]));
                for c in &r.changes {
                    let _ = writeln!(t, "    {}: {} -> {}", c.field, c.from, c.to);
                }
                Ok(())
            }
            (Op::Removed, Some(i), _, Some(j)) => {
                writeln!(t, "< {na}#{} moved to {nb}#{} {}", i + 1, j + 1, step(&a.steps[i]))
            }
            (Op::Removed, Some(i), _, None) => writeln!(t, "- {na}#{} {}", i + 1, step(&a.steps[i])),
            (Op::Added, _, Some(j), Some(i)) => {
                let _ = writeln!(t, "> {nb}#{} moved from {na}#{} {}", j + 1, i + 1, step(&b.steps[j]));
                for c in &r.changes {
                    let _ = writeln!(t, "    {}: {} -> {}", c.field, c.from, c.to);
                }
                Ok(())
            }
            (Op::Added, _, Some(j), None) => writeln!(t, "+ {nb}#{} {}", j + 1, step(&b.steps[j])),
            _ => Ok(()),
        };
        k += 1;
    }
}

fn zone_name(blk: &BlockDiff) -> &str {
    blk.zone.as_deref().unwrap_or("(before any zone)")
}

fn level_range(from: i64, to: i64) -> String {
    if from == to {
        format!("level {from}")
    } else {
        format!("levels {from}-{to}")
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

fn signed_time(seconds: f64) -> String {
    if seconds.abs() < 60.0 {
        "same".to_owned()
    } else if seconds < 0.0 {
        format!("-{}", fmt_time(-seconds))
    } else {
        format!("+{}", fmt_time(seconds))
    }
}

/// The parts of the time and XP that differ.
fn breakdown_changes(a: &Breakdown, b: &Breakdown) -> Vec<String> {
    let times = [
        ("travel", a.travel, b.travel),
        ("flights", a.flights, b.flights),
        ("fighting", a.fighting, b.fighting),
        ("grinding", a.grinding, b.grinding),
        ("farming", a.farming, b.farming),
        ("dungeons", a.dungeons, b.dungeons),
        ("training", a.training, b.training),
        ("overhead", a.overhead, b.overhead),
    ];
    let counts = [
        ("dungeon runs", i64::from(a.dungeon_runs), i64::from(b.dungeon_runs)),
        ("hearths", i64::from(a.hearths), i64::from(b.hearths)),
        (
            "class quests missed",
            i64::from(a.missing_class_quests),
            i64::from(b.missing_class_quests),
        ),
        (
            "zone quests missed",
            i64::from(a.missing_zone_quests),
            i64::from(b.missing_zone_quests),
        ),
        ("quest xp", a.quest_xp, b.quest_xp),
        ("mob xp", a.mob_xp, b.mob_xp),
        ("grind xp", a.grind_xp, b.grind_xp),
        ("farm xp", a.farm_xp, b.farm_xp),
    ];
    times
        .iter()
        .filter(|(_, x, y)| (x - y).abs() >= 60.0)
        .map(|(k, x, y)| format!("{k} {} -> {} ({})", fmt_time(*x), fmt_time(*y), signed_time(y - x)))
        .chain(
            counts
                .iter()
                .filter(|(_, x, y)| x != y)
                .map(|(k, x, y)| format!("{k} {x} -> {y}")),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::types::{MapPointOut, Target, TargetKind};
    use crate::faction::Faction;

    /// A step: kind, quest (or target for travel), zone.
    fn step(kind: StepKind, id: i64, zone: i64) -> Step {
        let quest = matches!(kind, StepKind::Accept | StepKind::Objective | StepKind::TurnIn).then_some(id);
        let target = quest.is_none().then(|| Target {
            kind: TargetKind::Taxi,
            id,
            name: format!("place {id}"),
        });
        Step {
            kind,
            quest,
            quest_name: quest.map(|q| format!("quest {q}")),
            alt: Vec::new(),
            pre: Vec::new(),
            pre_any: Vec::new(),
            pre_need: None,
            min_level: None,
            bg: false,
            try_buy: false,
            buy: false,
            inside: false,
            held: false,
            objective: (kind == StepKind::Objective).then_some(0),
            slot: None,
            child: None,
            child_name: None,
            text: format!("{} {id}", kind.as_str()),
            say: Vec::new(),
            target,
            zone: Some(format!("zone {zone}")),
            zone_id: Some(zone),
            map: Some(MapPointOut {
                ui_map: zone,
                x: 50.0,
                y: 50.0,
            }),
            world: None,
            height: None,
            path: Vec::new(),
            skill: None,
            line: None,
            mobs: Vec::new(),
            mob_ids: Vec::new(),
            elite: false,
            mob_level: None,
            reward: None,
            reward_xp: None,
            route_xp: None,
            spells: Vec::new(),
            xp: None,
            farm: None,
            farm_xp: None,
            level: 10,
            time: 0.0,
            power: None,
            gear: None,
            party: Vec::new(),
            members: None,
            keep: false,
            event: None,
            spots: Vec::new(),
            patrol: false,
        }
    }

    /// A quest done in a zone: accept, objective, turn-in.
    fn quest(id: i64, zone: i64) -> Vec<Step> {
        vec![
            step(StepKind::Accept, id, zone),
            step(StepKind::Objective, id, zone),
            step(StepKind::TurnIn, id, zone),
        ]
    }

    fn route(steps: Vec<Step>) -> Route {
        let mut steps = steps;
        for (i, s) in steps.iter_mut().enumerate() {
            // Times and levels follow from the steps before: they shift with every change.
            s.time = i as f64 * 60.0;
            s.level = 10 + i as i64 / 4;
        }
        Route {
            format: crate::export::FORMAT,
            name: "test".into(),
            race_id: 1,
            class_id: 1,
            class: "WARRIOR".into(),
            faction: Faction::Alliance,
            from_level: 10,
            to_level: 20,
            total_time: steps.len() as f64 * 60.0,
            total_time_text: String::new(),
            quests: 0,
            breakdown: Breakdown::default(),
            professions: Vec::new(),
            locale: None,
            spells: Vec::new(),
            checkpoints: Vec::new(),
            reputation: Vec::new(),
            players: Vec::new(),
            sync_max: None,
            class_spells: Vec::new(),
            steps,
        }
    }

    fn zone(quests: std::ops::Range<i64>, zone: i64) -> Vec<Step> {
        quests.flat_map(|q| quest(q, zone)).collect()
    }

    fn ops(d: &RouteDiff) -> Vec<Op> {
        d.blocks
            .iter()
            .flat_map(|b| b.rows.iter().map(|r| r.op))
            .filter(|o| *o != Op::Same)
            .collect()
    }

    #[test]
    fn same_guide_has_no_difference() {
        let r = route([zone(1..5, 1), zone(5..9, 2)].concat());
        let d = diff(&r, &r);
        assert!(d.summary.is_empty());
        assert!(d.blocks.iter().all(|b| b.status == BlockStatus::Same));
        assert!(to_text(&d, &r, &r, ["v1", "v2"], &[]).contains("Same steps."));
    }

    #[test]
    fn a_step_added_is_one_line_not_a_shift() {
        let a = route([zone(1..10, 1), zone(10..20, 2)].concat());
        let mut steps = a.steps.clone();
        steps.insert(2, step(StepKind::FlightMaster, 77, 1));
        let b = route(steps);
        let d = diff(&a, &b);
        assert_eq!(ops(&d), [Op::Added]);
        assert_eq!(d.summary.added, 1);
        // Every later step is done later and at another level, and is still the same step.
        assert_eq!(d.summary.changed, 0);
    }

    #[test]
    fn a_step_removed_and_one_changed() {
        let a = route(zone(1..10, 1));
        let mut steps = a.steps.clone();
        steps.remove(4);
        steps[7].text = "objective 3 x10".into();
        let b = route(steps);
        let d = diff(&a, &b);
        assert_eq!(ops(&d), [Op::Removed, Op::Changed]);
        let changed = d.blocks[0].rows.iter().find(|r| r.op == Op::Changed).unwrap();
        assert_eq!(changed.changes[0].field, "text");
    }

    #[test]
    fn two_zones_swapped_are_one_move() {
        // The smaller zone is the one that moved.
        let (x, y, z) = (zone(1..8, 1), zone(8..12, 2), zone(12..16, 3));
        let a = route([x.clone(), y.clone(), z.clone()].concat());
        let mut y2 = y;
        y2[1].text = "objective 8 elsewhere".into();
        let b = route([y2, x, z].concat());
        let d = diff(&a, &b);
        let statuses: Vec<BlockStatus> = d.blocks.iter().map(|b| b.status).collect();
        assert_eq!(d.summary.blocks_moved, 1);
        assert_eq!(d.summary.added + d.summary.removed + d.summary.moved, 0);
        // The moved zone keeps the difference of its steps.
        assert_eq!(d.summary.changed, 1);
        assert_eq!(statuses.iter().filter(|s| **s == BlockStatus::MovedAway).count(), 1);
        assert_eq!(statuses.iter().filter(|s| **s == BlockStatus::Same).count(), 2);
        let text = to_text(&d, &a, &b, ["v1", "v2"], &[]);
        assert!(text.contains("@@ moved here: zone "), "{text}");
    }

    #[test]
    fn a_step_done_in_another_zone_is_moved() {
        let mut x = zone(1..5, 1);
        let mut y = zone(5..9, 2);
        // The turn-in of quest 1 waits for the next zone (its NPC stands at the border).
        let mut turn_in = x.remove(2);
        turn_in.zone_id = Some(2);
        y.insert(0, turn_in);
        let a = route([zone(1..5, 1), zone(5..9, 2)].concat());
        let b = route([x, y].concat());
        let d = diff(&a, &b);
        assert_eq!(d.summary.moved, 1);
        assert_eq!(d.summary.added + d.summary.removed, 0);
        let moved = d
            .blocks
            .iter()
            .flat_map(|b| &b.rows)
            .find(|r| r.op == Op::Removed)
            .unwrap();
        assert_eq!(moved.moved, Some(11));
    }

    #[test]
    fn a_hearthstone_elsewhere_is_another_one() {
        let mut steps = zone(1..5, 1);
        steps.insert(3, step(StepKind::Hearth, 9, 1));
        let a = route(steps);
        let mut steps = zone(1..5, 1);
        steps.insert(9, step(StepKind::Hearth, 9, 1));
        let b = route(steps);
        let d = diff(&a, &b);
        assert_eq!((d.summary.added, d.summary.removed, d.summary.moved), (1, 1, 0));
    }

    #[test]
    fn a_new_zone_and_a_zone_dropped() {
        let a = route([zone(1..4, 1), zone(4..8, 2)].concat());
        let b = route([zone(1..4, 1), zone(20..23, 5)].concat());
        let d = diff(&a, &b);
        assert_eq!((d.summary.blocks_added, d.summary.blocks_removed), (1, 1));
        assert_eq!((d.summary.added, d.summary.removed), (9, 12));
    }

    #[test]
    fn a_spot_moved_is_a_change() {
        let a = route(zone(1..4, 1));
        let mut steps = a.steps.clone();
        steps[4].map.as_mut().unwrap().x = 70.0;
        steps[5].map.as_mut().unwrap().x = 50.5;
        let d = diff(&a, &route(steps));
        assert_eq!(d.summary.changed, 1);
        assert_eq!(d.blocks[0].rows[4].changes[0].field, "position");
    }

    #[test]
    fn settings_changed() {
        let a = serde_json::json!({ "race": "human", "params": { "progression": "cautious", "effort": 1.0 } });
        let b = serde_json::json!({ "race": "human", "params": { "progression": "normal", "effort": 1.0 }, "group": ["mage"] });
        let c = value_changes(&a, &b);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].field.as_str(), c[0].to.as_str()), ("group", r#"["mage"]"#));
        assert_eq!(
            (c[1].field.as_str(), c[1].from.as_str()),
            ("params.progression", "cautious")
        );
    }
}
