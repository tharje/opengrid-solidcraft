//! openGrid plugin for SolidCraft: entry `board` builds an openGrid wall board (Full or Lite)
//! as ordinary, editable SolidCraft features made through the host's command API.
//!
//! The tile geometry follows openGrid by David D (CC BY 4.0), with the dimensions of the
//! openGrid-Studio generator's `og_openGridTileAp1` (Apache 2.0): a 28 mm grid, a 25 mm opening
//! that widens to 26.4 mm inside, 45° chamfers at the faces and a corner block in each corner.

use serde_json::{json, Map, Value};

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({ world: "command-plugin", path: "wit" });
}

/// Grid size (mm).
pub const TILE: f64 = 28.0;
/// Board thickness (mm).
pub const FULL_T: f64 = 6.8;
pub const LITE_T: f64 = 4.0;
/// The opening through a Full tile, from the back up: height, how far the wall reaches in from
/// the tile's edge, and how far the corner block reaches from the tile's corner, measured
/// square to its face (mm). Between two heights both change in a straight line.
const FULL: [(f64, f64, f64); 8] = [
    (0.0, 1.1, 4.17),
    (0.4, 1.5, 4.57),
    (1.4, 1.5, 5.57),
    (2.4, 0.8, 5.57),
    (4.4, 0.8, 5.57),
    (5.4, 1.5, 5.57),
    (6.4, 1.5, 4.57),
    (6.8, 1.1, 4.17),
];
/// A Lite board is the top 4 mm of a Full one: flat at the back, the same front.
fn sections(lite: bool) -> Vec<(f64, f64, f64)> {
    if !lite {
        return FULL.to_vec();
    }
    let cut = FULL_T - LITE_T;
    let mut s = vec![(0.0, 0.8, 5.57)];
    s.extend(FULL.iter().filter(|r| r.0 > cut + 1e-9).map(|&(z, w, c)| (r4(z - cut), w, c)));
    s
}
/// The outer corners of the board are cut off this far along each edge (mm).
const CORNER_CUT: f64 = 4.2;
/// Screw holes at the grid's nodes: a countersunk M4 (or #8) wood screw.
const SCREW_D: f64 = 4.1;
const HEAD_D: f64 = 7.2;
const HEAD_INSET: f64 = 1.0;
/// Most tiles in one board, and the longest a run may be expected to take (s): SolidCraft gives
/// a plugin 30 seconds. See `estimate`.
pub const MAX_TILES: u32 = 100;
const MAX_SECONDS: f64 = 27.0;
pub const MAX_SIDE: u32 = 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub nx: u32,
    pub ny: u32,
    pub lite: bool,
    /// Screw holes every this many nodes (0: none).
    pub screws: u32,
    pub chamfer: bool,
    pub origin: [f64; 2],
    pub name: String,
}

impl Options {
    pub fn thickness(&self) -> f64 {
        if self.lite { LITE_T } else { FULL_T }
    }
}

fn get_count(p: &Map<String, Value>, key: &str, default: u32, max: u32) -> Result<u32, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= max as f64).map(|n| n as u32)
            .ok_or(format!("{key} must be a whole number from 1 to {max}")),
    }
}

fn get_b(p: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(format!("{key} must be true or false")),
    }
}

fn get_choice<'a>(p: &'a Map<String, Value>, key: &str, default: &'a str) -> Result<&'a str, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::String(s)) => Ok(s),
        _ => Err(format!("{key} must be text")),
    }
}

fn get_origin(p: &Map<String, Value>) -> Result<[f64; 2], String> {
    let num = |key: &str| match p.get(key) {
        None | Some(Value::Null) => Ok(0.0),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).ok_or(format!("{key} must be a number (mm)")),
    };
    Ok([num("position_x")?, num("position_y")?])
}

pub fn parse(params: &Value) -> Result<Options, String> {
    let empty = Map::new();
    let p = params.as_object().unwrap_or(&empty);
    let nx = get_count(p, "width", 4, MAX_SIDE)?;
    let ny = get_count(p, "height", 4, MAX_SIDE)?;
    let lite = match get_choice(p, "board", "Full")?.to_ascii_lowercase() {
        s if s.starts_with("full") => false,
        s if s.starts_with("lite") => true,
        _ => return Err("board must be \"Full\" or \"Lite\"".into()),
    };
    let screws = match get_choice(p, "screw_holes", "None")?.to_ascii_lowercase() {
        s if s.starts_with("none") => 0,
        s if s.starts_with("every node") => 1,
        s if s.starts_with("every 2") => 2,
        s if s.starts_with("every 3") => 3,
        _ => return Err("screw_holes must be \"None\", \"Every node\", \"Every 2nd node\" or \"Every 3rd node\"".into()),
    };
    let name = match get_choice(p, "title", "")?.trim() {
        "" => format!("openGrid {}{nx}x{ny}", if lite { "Lite " } else { "" }),
        s if s.chars().count() > 60 => return Err("title is too long (at most 60 characters)".into()),
        s => s.to_string(),
    };
    Ok(Options { nx, ny, lite, screws, chamfer: get_b(p, "corner_chamfers", true)?, origin: get_origin(p)?, name })
}

pub fn r4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

fn cmd(command: &str, params: Value) -> Value {
    json!({"command": command, "params": params})
}

fn sketch(out: &mut Vec<Value>, z: f64, name: &str, items: Vec<Value>) {
    out.push(cmd("sketch.create", json!({"plane": "XY", "offset": r4(z), "name": name})));
    out.extend(items);
    out.push(cmd("sketch.finish", json!({})));
}

/// The opening of the tile whose corner is at `o`, at one height: a square `w` in from each
/// edge with its corners cut where the corner blocks stand, `c` from the tile's corners.
fn opening(o: [f64; 2], w: f64, c: f64) -> Value {
    let s = c * std::f64::consts::SQRT_2;
    let p = [(w, s - w), (s - w, w), (TILE - s + w, w), (TILE - w, s - w), (TILE - w, TILE - s + w),
        (TILE - s + w, TILE - w), (s - w, TILE - w), (w, TILE - s + w)];
    let pts: Vec<Value> = p.iter().map(|(x, y)| json!([r4(o[0] + x), r4(o[1] + y)])).collect();
    cmd("sketch.line", json!({"points": pts, "closed": true}))
}

/// The grid nodes that get a screw hole along one side of `n` tiles: inner nodes only, every
/// `every`th, starting at the first.
pub fn screw_nodes(n: u32, every: u32) -> Vec<u32> {
    if every == 0 {
        return vec![];
    }
    (1..n).filter(|i| (i - 1) % every == 0).collect()
}

/// One tile is built and copied into a row, and the row up into the others: much faster than
/// cutting every opening into a whole board. Screw holes and cut corners are cut into rows or
/// blocks of rows while they are small, and those are joined last: SolidCraft takes seconds for
/// every change to a big body (in the desktop app a corner cut on a finished 8x8 board alone
/// took 15 s).
pub fn commands(o: &Options) -> Vec<Value> {
    let mut out = vec![];
    let b = o.name.as_str();
    let n = |s: &str| format!("{b} {s}");
    let [x0, y0] = o.origin;
    let t = o.thickness();
    let sec = sections(o.lite);

    out.push(cmd("solid.box", json!({"corner": [r4(x0), r4(y0), 0.0], "length": TILE, "width": TILE, "height": t, "body_name": b})));
    for (i, &(z, w, c)) in sec.iter().enumerate() {
        sketch(&mut out, z, &n(&format!("Opening {}", i + 1)), vec![opening([x0, y0], w, c)]);
    }
    for i in 0..sec.len() - 1 {
        out.push(cmd("solid.loft", json!({"sections": [{"sketch": n(&format!("Opening {}", i + 1))},
            {"sketch": n(&format!("Opening {}", i + 2))}], "operation": "cut", "targets": [b]})));
    }
    let copies = |body: &str, dir: [i32; 3], count: usize, spacing: f64, save: &str| cmd("solid.pattern.rectangular",
        json!({"bodies": [body], "dir1": dir, "count1": count, "spacing1": r4(spacing), "save": save}));
    if o.nx > 1 {
        out.push(copies(b, [1, 0, 0], o.nx as usize, TILE, "tiles"));
        out.push(cmd("solid.combine", json!({"target": b, "tools": ["@tiles:*"], "operation": "join"})));
    }
    let (sx, sy) = (screw_nodes(o.nx, o.screws), screw_nodes(o.ny, o.screws));
    let holes = !sx.is_empty() && !sy.is_empty();
    let h = n("Screw holes");
    if holes {
        // one screw hole, on the first node of the first row of nodes, copied along that row
        let (x, y) = (r4(x0 + sx[0] as f64 * TILE), r4(y0 + sy[0] as f64 * TILE));
        // the head sits HEAD_INSET below the face, on a 90° countersink down to the shank
        out.push(cmd("solid.cylinder", json!({"diameter": HEAD_D, "height": HEAD_INSET, "base": [x, y, r4(t - HEAD_INSET)],
            "operation": "new", "body_name": h})));
        out.push(cmd("solid.cone", json!({"diameter": HEAD_D, "top_diameter": SCREW_D, "height": -r4((HEAD_D - SCREW_D) / 2.0),
            "base": [x, y, r4(t - HEAD_INSET)], "operation": "join", "targets": [h]})));
        out.push(cmd("solid.cylinder", json!({"diameter": SCREW_D, "height": t, "base": [x, y, 0.0], "operation": "join", "targets": [h]})));
        if sx.len() > 1 {
            out.push(copies(&h, [1, 0, 0], sx.len(), o.screws as f64 * TILE, "hx"));
            out.push(cmd("solid.combine", json!({"target": h, "tools": ["@hx:*"], "operation": "join"})));
        }
    }

    // the corners: two at the front edge, two at the back
    let (front, back) = (n("Corners front"), n("Corners back"));
    if o.chamfer {
        let (x1, y1, k) = (x0 + o.nx as f64 * TILE, y0 + o.ny as f64 * TILE, CORNER_CUT);
        let tri = |x: f64, y: f64, dx: f64, dy: f64| cmd("sketch.line", json!({"points":
            [[r4(x), r4(y)], [r4(x + dx * k), r4(y)], [r4(x), r4(y + dy * k)]], "closed": true}));
        for (name, y, dy) in [(&front, y0, 1.0), (&back, y1, -1.0)] {
            sketch(&mut out, 0.0, name, vec![tri(x0, y, 1.0, dy), tri(x1, y, -1.0, dy)]);
            // two separate profiles: two bodies, both cut below
            out.push(cmd("solid.extrude", json!({"sketch": name, "profiles": "all", "distance": t, "operation": "new", "body_name": name})));
        }
    }
    let cut = |out: &mut Vec<Value>, target: &str, tools: Vec<String>, keep: bool| if !tools.is_empty() {
        out.push(cmd("solid.combine", json!({"target": target, "tools": tools, "operation": "cut", "keep_tools": keep})));
    };
    let corners = |r_front: bool, r_back: bool| {
        let mut v = vec![];
        if o.chamfer && r_front { v.push(front.clone()); }
        if o.chamfer && r_back { v.push(back.clone()); }
        v
    };

    if holes && o.screws > 1 {
        // Blocks of `screws` rows, each with one row of holes inside it, so no hole falls on a
        // seam between blocks: joining two halves of holes is slow. The first block is built
        // and copied up; rows left over make a last, shorter block.
        let k = o.screws.min(o.ny);
        let (q, m) = (o.ny / k, o.ny % k);
        if m > 0 {
            out.push(copies(b, [0, 1, 0], 2, (q * k) as f64 * TILE, "rest"));
            if m > 1 {
                out.push(copies("@rest:0", [0, 1, 0], m as usize, TILE, "restrows"));
                out.push(cmd("solid.combine", json!({"target": "@rest:0", "tools": ["@restrows:*"], "operation": "join"})));
            }
        }
        if k > 1 {
            out.push(copies(b, [0, 1, 0], k as usize, TILE, "rows"));
            out.push(cmd("solid.combine", json!({"target": b, "tools": ["@rows:*"], "operation": "join"})));
        }
        if m > 1 {
            // the leftover block's holes: one row of nodes above its first row
            out.push(copies(&h, [0, 1, 0], 2, (q * k) as f64 * TILE, "hrest"));
            cut(&mut out, "@rest:0", vec!["@hrest:0".into()], false);
        }
        cut(&mut out, b, vec![h.clone()], false);
        if q > 1 {
            out.push(copies(b, [0, 1, 0], q as usize, (k as f64) * TILE, "blocks"));
        }
        let top = if m > 0 { "@rest:0".to_string() } else if q > 1 { format!("@blocks:{}", q - 2) } else { b.to_string() };
        if top == b {
            cut(&mut out, b, corners(true, true), false);
        } else {
            cut(&mut out, b, corners(true, false), false);
            cut(&mut out, &top, corners(false, true), false);
        }
        let mut rest = vec![];
        if q > 1 { rest.push("@blocks:*".to_string()); }
        if m > 0 { rest.push("@rest:0".to_string()); }
        if !rest.is_empty() {
            out.push(cmd("solid.combine", json!({"target": b, "tools": rest, "operation": "join"})));
        }
    } else {
        // Row by row: each row gets what falls on it (a hole on the seam between two rows half
        // each) while the rows are still small, and the rows are joined last.
        if o.ny > 1 {
            out.push(copies(b, [0, 1, 0], o.ny as usize, TILE, "rows"));
        }
        let row = |r: u32| if r == 0 { b.to_string() } else { format!("@rows:{}", r - 1) };
        let mut spent = vec![];
        if holes && sy.len() > 1 {
            out.push(copies(&h, [0, 1, 0], sy.len(), o.screws as f64 * TILE, "hy"));
            spent.push(json!("@hy:*"));
        }
        if holes {
            spent.push(json!(h));
        }
        if o.chamfer {
            spent.push(json!(front));
            spent.push(json!(back));
        }
        let hole_row = |node: u32| sy.iter().filter(|_| holes).position(|&j| j == node)
            .map(|i| if i == 0 { h.clone() } else { format!("@hy:{}", i - 1) });
        for r in 0..o.ny {
            let mut tools: Vec<String> = [hole_row(r), hole_row(r + 1)].into_iter().flatten().collect();
            tools.extend(corners(r == 0, r == o.ny - 1));
            cut(&mut out, &row(r), tools, true);
        }
        if !spent.is_empty() {
            out.push(cmd("solid.remove", json!({"bodies": spent})));
        }
        if o.ny > 1 {
            out.push(cmd("solid.combine", json!({"target": b, "tools": ["@rows:*"], "operation": "join"})));
        }
    }
    out
}

/// Host calls for a command list: each command, and a progress and a cancel check every 16,
/// and the look at the design's bodies first.
pub fn calls(c: &[Value]) -> usize {
    1 + c.len() + 2 * c.len().div_ceil(16)
}

/// About how many seconds a board takes in the desktop app (SolidCraft 0.42 on Linux, software
/// rendering, measured): Full 8x8 13 s, 10x10 20 s; Lite 8x8 8 s; with holes every 2nd or 3rd
/// node, Full 8x8 20 to 26 s, Lite 8x8 14 s; Lite 6x6 with a hole on every node 20 s, Full 26 s.
pub fn estimate(o: &Options) -> f64 {
    let tiles = (o.nx * o.ny) as f64;
    let per_tile = if o.lite { 0.12 } else { 0.2 };
    let holes = (screw_nodes(o.nx, o.screws).len() * screw_nodes(o.ny, o.screws).len()) as f64;
    let for_holes = match o.screws {
        _ if holes == 0.0 => 0.0,
        // a hole on every node: half of each is cut into the row on either side, and joining
        // the halves again is slow
        1 => 0.75 * holes,
        // blocks of rows with the holes inside them
        _ => per_tile * tiles,
    };
    0.5 + per_tile * tiles + for_holes
}

/// The command list for an entry and the body it makes, or what is wrong with the input.
pub fn plan(entry: &str, params: &Value) -> Result<(String, Vec<Value>), String> {
    match entry {
        "board" => {
            let o = parse(params)?;
            let tiles = o.nx * o.ny;
            if tiles > MAX_TILES {
                return Err(format!("{}x{} is {tiles} tiles, and SolidCraft gives a plugin 30 seconds, which is about \
                    {MAX_TILES} tiles. Make it in smaller boards: they print better anyway.", o.nx, o.ny));
            }
            if estimate(&o) > MAX_SECONDS {
                let fewer = if o.screws == 1 { "screw holes on every 2nd or 3rd node, " } else { "" };
                return Err(format!("A {}x{} {} board{} takes longer to build than SolidCraft gives a plugin (30 seconds). \
                    Use {fewer}a smaller board, or a Lite board.", o.nx, o.ny, if o.lite { "Lite" } else { "Full" },
                    if o.screws > 0 { " with these screw holes" } else { "" }).replace(", or a Lite board", if o.lite { "" } else { ", or a Lite board" }));
            }
            Ok((o.name.clone(), commands(&o)))
        }
        other => Err(format!("unknown entry `{other}` (use `board`)")),
    }
}

/// A name for a new body called `name` that no body in the design has, nor its parts:
/// `name` itself if it is free, else `name (2)`, `name (3)`, ...
pub fn free_name(name: &str, existing: &[String]) -> Option<String> {
    let taken = |n: &str| existing.iter().any(|e| e.eq_ignore_ascii_case(n)
        || e.to_lowercase().starts_with(&format!("{} ", n.to_lowercase())));
    if !taken(name) {
        return None;
    }
    (2..).map(|k| format!("{name} ({k})")).find(|n| !taken(n))
}

/// Turns the plan's commands into the ones SolidCraft runs, using what earlier ones reported:
/// `@pattern` becomes `sketch.pattern.rectangular` over the curves drawn since the sketch began,
/// `@rows:*` in a body list the bodies a body pattern made and saved as `rows`, and body and sketch names
/// the ones SolidCraft actually gave.
#[derive(Default)]
pub struct Runner {
    drawn: Vec<Value>,
    /// The bodies each body pattern made, by the name the plan saved them under: `@rows:*` is
    /// all of them, `@rows:2` the third.
    saved: std::collections::HashMap<String, Vec<Value>>,
    /// Each body the plan asked for, and the bodies SolidCraft made for it: an extrude of
    /// separate profiles makes one body each.
    bodies: std::collections::HashMap<String, Vec<Value>>,
    sketches: std::collections::HashMap<String, Value>,
    /// Every sketch and feature the run made, in order, for the folder they go in.
    pub features: Vec<Value>,
}

impl Runner {
    /// The bodies a name in the plan stands for.
    fn resolve(&self, s: &str) -> Vec<Value> {
        if let Some((key, which)) = s.strip_prefix('@').and_then(|r| r.split_once(':')) {
            let list = self.saved.get(key).cloned().unwrap_or_default();
            return match which.parse::<usize>() {
                Ok(i) => list.get(i).cloned().into_iter().collect(),
                Err(_) => list,
            };
        }
        self.bodies.get(s).cloned().unwrap_or_else(|| vec![json!(s)])
    }

    pub fn prepare(&self, c: &Value) -> (String, Value) {
        let name = c["command"].as_str().unwrap_or_default();
        if name == "@pattern" {
            return ("sketch.pattern.rectangular".into(), pattern_args(&c["params"], &self.drawn));
        }
        let mut p = c["params"].clone();
        for key in ["targets", "bodies", "tools"] {
            if let Some(Value::Array(list)) = p.get(key) {
                let mut out = Vec::new();
                for v in list {
                    match v.as_str() {
                        Some(s) => out.extend(self.resolve(s)),
                        None => out.push(v.clone()),
                    }
                }
                p[key] = json!(out);
            }
        }
        if let Some(s) = p.get("target").and_then(|v| v.as_str()) {
            p["target"] = self.resolve(s).into_iter().next().unwrap_or(json!(s));
        }
        if let Some(m) = p.as_object_mut() {
            m.remove("save");
        }
        if let Some(id) = p.get("sketch").and_then(|v| v.as_str()).and_then(|s| self.sketches.get(s)) {
            p["sketch"] = id.clone();
        }
        if let Some(Value::Array(list)) = p.get_mut("sections") {
            for s in list {
                if let Some(id) = s.get("sketch").and_then(|v| v.as_str()).and_then(|n| self.sketches.get(n)) {
                    s["sketch"] = id.clone();
                }
            }
        }
        (name.to_string(), p)
    }

    pub fn record(&mut self, c: &Value, reply: &Value) {
        let name = c["command"].as_str().unwrap_or_default();
        let reply = reply.get("result").filter(|r| r.is_object()).unwrap_or(reply);
        let made = if name == "sketch.create" { reply.get("sketch") } else { reply.get("feature") };
        if let Some(id) = made.filter(|id| id.is_number()) {
            self.features.push(id.clone());
        }
        match name {
            "@pattern" => self.drawn.clear(),
            "sketch.create" => {
                self.drawn.clear();
                if let (Some(n), Some(id)) = (c["params"]["name"].as_str(), reply.get("sketch")) {
                    self.sketches.insert(n.to_string(), id.clone());
                }
            }
            "sketch.finish" => {}
            n if n.starts_with("sketch.") => {
                if let Some(Value::Array(ids)) = reply.pointer("/created/curves") {
                    self.drawn.extend(ids.iter().cloned());
                }
            }
            _ => {}
        }
        if let Some(Value::Array(made)) = reply.get("created_bodies") {
            if let Some(key) = c["params"]["save"].as_str() {
                self.saved.insert(key.to_string(), made.clone());
            }
            if let Some(asked) = c["params"]["body_name"].as_str().filter(|_| !made.is_empty()) {
                self.bodies.insert(asked.to_string(), made.clone());
            }
        }
    }
}

/// `sketch.pattern.rectangular`'s arguments for an `@pattern` step over the curves `ids`.
pub fn pattern_args(p: &Value, ids: &[Value]) -> Value {
    let (n1, n2) = (p["count"].as_u64().unwrap_or(1), p["count2"].as_u64().unwrap_or(1));
    if n1 == 1 {
        json!({"entities": ids, "count": n2, "spacing": p["spacing2"], "dir": [0, 1]})
    } else if n2 == 1 {
        json!({"entities": ids, "count": n1, "spacing": p["spacing"]})
    } else {
        json!({"entities": ids, "count": n1, "spacing": p["spacing"], "count2": n2, "spacing2": p["spacing2"]})
    }
}

#[cfg(target_arch = "wasm32")]
mod guest {
    use super::*;
    use bindings::craft::plugin::commands;
    use bindings::craft::plugin::control;
    use bindings::craft::plugin::types::{FieldFailure, PluginError};
    use bindings::exports::craft::plugin::entry::Guest;

    struct OpenGrid;

    impl Guest for OpenGrid {
        fn execute(entry: String, params: String) -> Result<String, PluginError> {
            let params: Value = serde_json::from_str(if params.trim().is_empty() { "{}" } else { &params })
                .map_err(|e| PluginError::InvalidInput(FieldFailure { field: "params".into(), reason: e.to_string() }))?;
            let invalid = |reason| PluginError::InvalidInput(FieldFailure { field: entry.clone(), reason });
            let (mut body, mut cmds) = plan(&entry, &params).map_err(invalid)?;
            // A body named like one already in the design would get an automatic name that
            // SolidCraft doesn't keep when it rebuilds: give it a free name of its own instead.
            let list: Value = serde_json::from_str(&commands::run("model.body_features", &json!({}).to_string())?).unwrap_or(Value::Null);
            let list = list.get("result").unwrap_or(&list);
            let existing: Vec<String> = list["bodies"].as_array().into_iter().flatten()
                .filter_map(|b| b["body"].as_str().map(String::from)).collect();
            if let Some(free) = free_name(&body, &existing) {
                let mut p = params.clone();
                p["title"] = json!(free);
                (body, cmds) = plan(&entry, &p).map_err(invalid)?;
            }
            let total = cmds.len();
            let mut run = Runner::default();
            for (i, c) in cmds.iter().enumerate() {
                // progress and cancel checks are host calls too, and a run may make only 256
                if i % 16 == 0 {
                    if control::cancelled() {
                        return Err(PluginError::Cancelled);
                    }
                    control::progress(i as f32 / total as f32, &format!("openGrid: step {} of {total}", i + 1));
                }
                let (name, args) = run.prepare(c);
                let reply = commands::run(&name, &args.to_string())?;
                run.record(c, &serde_json::from_str(&reply).unwrap_or(Value::Null));
            }
            // One folder in the timeline, named like the board, holds everything the run made.
            // The board is finished by now, so a folder that can't be made doesn't fail the run.
            if !run.features.is_empty() {
                let _ = commands::run("timeline.group", &json!({"name": body, "features": run.features}).to_string());
            }
            Ok(json!({"body": body, "commands": total}).to_string())
        }
    }

    bindings::export!(OpenGrid with_types_in bindings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runner_collects_what_goes_in_the_folder() {
        let mut run = Runner::default();
        run.record(&json!({"command": "sketch.create", "params": {"name": "S"}}), &json!({"sketch": 1}));
        run.record(&json!({"command": "sketch.line", "params": {}}), &json!({"created": {"curves": ["l1"]}}));
        run.record(&json!({"command": "solid.loft", "params": {}}), &json!({"result": {"feature": 2}}));
        assert_eq!(run.features, vec![json!(1), json!(2)]);
    }

    #[test]
    fn defaults() {
        let o = parse(&json!({})).unwrap();
        assert_eq!((o.nx, o.ny, o.lite, o.screws, o.chamfer), (4, 4, false, 0, true));
        assert_eq!(o.name, "openGrid 4x4");
        assert_eq!(parse(&json!({"board": "Lite", "width": 3, "height": 2})).unwrap().name, "openGrid Lite 3x2");
        assert!(parse(&json!({"width": 0})).is_err());
        assert!(parse(&json!({"width": 2.5})).is_err());
        assert!(parse(&json!({"board": "Heavy"})).is_err());
        assert!(plan("board", &json!({"width": 11, "height": 10})).unwrap_err().contains("smaller boards"));
    }

    #[test]
    fn time_limits() {
        let ok = |p: Value| plan("board", &p).is_ok();
        // measured in the desktop app: these all finished in 26 s or less
        assert!(ok(json!({"width": 10, "height": 10})));
        assert!(ok(json!({"width": 8, "height": 8, "screw_holes": "Every 3rd node"})));
        assert!(ok(json!({"width": 10, "height": 10, "board": "Lite", "screw_holes": "Every 2nd node"})));
        assert!(ok(json!({"width": 6, "height": 6, "board": "Lite", "screw_holes": "Every node"})));
        let e = plan("board", &json!({"width": 9, "height": 9, "screw_holes": "Every 2nd node"})).unwrap_err();
        assert!(e.contains("9x9 Full board with these screw holes") && e.contains("or a Lite board"), "{e}");
        let e = plan("board", &json!({"width": 8, "height": 8, "board": "Lite", "screw_holes": "Every node"})).unwrap_err();
        assert!(e.contains("every 2nd or 3rd") && !e.contains("or a Lite board"), "{e}");
    }

    #[test]
    fn lite_is_the_top_of_full() {
        let s = sections(true);
        assert_eq!(s.first().unwrap(), &(0.0, 0.8, 5.57));
        assert_eq!(s.last().unwrap(), &(4.0, 1.1, 4.17));
        assert_eq!(s.len(), 5);
        // the cut at 2.8 mm falls inside the straight middle of the Full opening
        assert_eq!(s[1], (1.6, 0.8, 5.57));
    }

    /// Material left in one tile, from the openings' cross-sections (exact: each piece is a
    /// prismatoid, which Simpson's rule integrates exactly).
    fn tile_volume(lite: bool) -> f64 {
        let area = |w: f64, c: f64| {
            let s = c * std::f64::consts::SQRT_2;
            (TILE - 2.0 * w).powi(2) - 2.0 * (s - 2.0 * w).powi(2)
        };
        let sec = sections(lite);
        let t = sec.last().unwrap().0;
        let hole: f64 = sec.windows(2).map(|p| {
            let ((z0, w0, c0), (z1, w1, c1)) = (p[0], p[1]);
            (z1 - z0) / 6.0 * (area(w0, c0) + 4.0 * area((w0 + w1) / 2.0, (c0 + c1) / 2.0) + area(w1, c1))
        }).sum();
        TILE * TILE * t - hole
    }

    #[test]
    fn tile_volumes() {
        // checked against SolidCraft's measurement of a built 2x2 board (4892.92 mm³)
        assert!((tile_volume(false) * 4.0 - 4892.92).abs() < 0.1);
        assert!(tile_volume(true) > 0.0 && tile_volume(true) < tile_volume(false));
    }

    #[test]
    fn one_tile_copied_into_rows_and_columns() {
        let c = commands(&parse(&json!({"width": 5, "height": 3})).unwrap());
        let pats: Vec<&Value> = c.iter().filter(|v| v["command"] == "solid.pattern.rectangular").collect();
        assert_eq!(pats.len(), 2);
        assert_eq!((pats[0]["params"]["count1"].as_u64(), pats[1]["params"]["count1"].as_u64()), (Some(5), Some(3)));
        assert_eq!(c.iter().filter(|v| v["command"] == "solid.loft").count(), 7);
        // a single tile has nothing to copy
        let one = commands(&parse(&json!({"width": 1, "height": 1})).unwrap());
        assert!(!one.iter().any(|v| v["command"] == "solid.pattern.rectangular"));
    }

    #[test]
    fn screw_holes_on_inner_nodes() {
        assert_eq!(screw_nodes(4, 1), vec![1, 2, 3]);
        assert_eq!(screw_nodes(4, 2), vec![1, 3]);
        assert_eq!(screw_nodes(6, 3), vec![1, 4]);
        assert_eq!(screw_nodes(1, 1), Vec::<u32>::new());
        let cuts = |c: &[Value]| -> Vec<(Value, Value)> { c.iter().filter(|v| v["params"]["operation"] == "cut" && v["command"] == "solid.combine")
            .map(|v| (v["params"]["target"].clone(), v["params"]["tools"].clone())).collect() };
        let (h, f, b) = ("openGrid Lite 3x3 Screw holes", "openGrid Lite 3x3 Corners front", "openGrid Lite 3x3 Corners back");

        // every node: row by row, row 0 gets the bottom half of node row 1, row 1 its top half, ...
        let c = commands(&parse(&json!({"board": "Lite", "width": 3, "height": 3, "screw_holes": "Every node"})).unwrap());
        let hx = c.iter().find(|v| v["params"]["save"] == "hx").unwrap();
        assert_eq!((hx["params"]["count1"].as_u64(), hx["params"]["spacing1"].as_f64()), (Some(2), Some(28.0)));
        assert_eq!(cuts(&c), vec![(json!("openGrid Lite 3x3"), json!([h, f])), (json!("@rows:0"), json!([h, "@hy:0"])),
            (json!("@rows:1"), json!(["@hy:0", b]))]);
        assert_eq!(c[c.len() - 2]["params"]["bodies"], json!(["@hy:*", h, f, b]));
        assert_eq!(c[c.len() - 1]["params"]["tools"], json!(["@rows:*"]));

        // every 2nd node on 5 rows: blocks of 2 rows with the holes inside, a last row of its own
        let c = commands(&parse(&json!({"width": 4, "height": 5, "screw_holes": "Every 2nd node"})).unwrap());
        let (h, f, b) = ("openGrid 4x5 Screw holes", "openGrid 4x5 Corners front", "openGrid 4x5 Corners back");
        assert_eq!(cuts(&c), vec![(json!("openGrid 4x5"), json!([h])), (json!("openGrid 4x5"), json!([f])), (json!("@rest:0"), json!([b]))]);
        let rest = c.iter().find(|v| v["params"]["save"] == "rest").unwrap();
        assert_eq!(rest["params"]["spacing1"].as_f64(), Some(112.0));
        let blocks = c.iter().find(|v| v["params"]["save"] == "blocks").unwrap();
        assert_eq!((blocks["params"]["count1"].as_u64(), blocks["params"]["spacing1"].as_f64()), (Some(2), Some(56.0)));
        assert_eq!(c[c.len() - 1]["params"]["tools"], json!(["@blocks:*", "@rest:0"]));
        assert!(!c.iter().any(|v| v["command"] == "solid.remove"));

        // every 3rd node on 5 rows: a block of 3 and one of 2, both with holes
        let c = commands(&parse(&json!({"width": 4, "height": 5, "screw_holes": "Every 3rd node", "corner_chamfers": false})).unwrap());
        assert_eq!(cuts(&c), vec![(json!("@rest:0"), json!(["@hrest:0"])), (json!("openGrid 4x5"), json!([h]))]);
        let hrest = c.iter().find(|v| v["params"]["save"] == "hrest").unwrap();
        assert_eq!(hrest["params"]["spacing1"].as_f64(), Some(84.0)); // node 4 = node 1 + 3 rows
        assert_eq!(c[c.len() - 1]["params"]["tools"], json!(["@rest:0"]));
        // no inner node, no holes
        let c = commands(&parse(&json!({"width": 1, "height": 3, "screw_holes": "Every node", "corner_chamfers": false})).unwrap());
        assert!(!c.iter().any(|v| v["command"] == "solid.combine" && v["params"]["operation"] == "cut"));
    }

    #[test]
    fn biggest_board_stays_within_the_call_limit() {
        let c = commands(&parse(&json!({"width": 10, "height": 10, "screw_holes": "Every node"})).unwrap());
        assert!(calls(&c) <= 256, "{}", calls(&c));
    }

    #[test]
    fn runner_uses_every_body_an_extrude_made() {
        let mut r = Runner::default();
        let make = cmd("solid.extrude", json!({"body_name": "B Corners", "operation": "new"}));
        r.record(&make, &json!({"created_bodies": ["B Corners", "B Corners (2)"]}));
        let pat = cmd("solid.pattern.rectangular", json!({"bodies": ["B"], "save": "rows"}));
        r.record(&pat, &json!({"created_bodies": ["B (2)", "B (3)"]}));
        let (_, p) = r.prepare(&cmd("solid.combine", json!({"target": "B", "tools": ["@rows:*", "B Corners"]})));
        assert_eq!(p["tools"], json!(["B (2)", "B (3)", "B Corners", "B Corners (2)"]));
        assert_eq!(p["target"], json!("B"));
        let (_, p) = r.prepare(&cmd("solid.combine", json!({"target": "@rows:1", "tools": ["B Corners"]})));
        assert_eq!(p["target"], json!("B (3)"));
        let (_, p) = r.prepare(&pat);
        assert!(p.get("save").is_none());
    }

    #[test]
    fn names() {
        assert_eq!(free_name("openGrid 4x4", &["openGrid 4x4".into()]), Some("openGrid 4x4 (2)".into()));
        assert_eq!(free_name("openGrid 4x4", &["Other".into()]), None);
    }
}
