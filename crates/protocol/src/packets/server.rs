//! Server -> Client packet building.

use crate::{BinaryWriter, Color};

/// Build a ClearAll packet (0x12).
pub fn build_clear_all() -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(1);
    w.put_u8(0x12);
    w
}

/// Build a ClearOwned packet (0x14).
pub fn build_clear_owned() -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(1);
    w.put_u8(0x14);
    w
}

/// Build an AddNode packet (0x20).
pub fn build_add_node(node_id: u32, scramble_id: u32) -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(5);
    w.put_u8(0x20);
    w.put_u32(node_id ^ scramble_id);
    w
}

/// Build a SetBorder packet (0x40).
pub fn build_set_border(
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    game_type: u32,
    server_name: &str,
) -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(33 + server_name.len() + 1);
    w.put_u8(0x40);
    w.put_f64(min_x);
    w.put_f64(min_y);
    w.put_f64(max_x);
    w.put_f64(max_y);
    w.put_u32(game_type);
    w.put_string_utf8(server_name);
    w
}

/// Build an UpdatePosition packet (0x11) for spectators.
pub fn build_update_position(x: f32, y: f32, scale: f32) -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(13);
    w.put_u8(0x11);
    w.put_f32(x);
    w.put_f32(y);
    w.put_f32(scale);
    w
}

/// Build a ChatMessage packet (0x63).
pub fn build_chat_message(
    color: Color,
    name: &str,
    message: &str,
    is_server: bool,
    is_admin: bool,
    is_mod: bool,
) -> BinaryWriter {
    let mut flags = 0u8;
    if is_server {
        flags |= 0x80;
    }
    if is_admin {
        flags |= 0x40;
    }
    if is_mod {
        flags |= 0x20;
    }

    let mut w = BinaryWriter::new();
    w.put_u8(0x63);
    w.put_u8(flags);
    w.put_u8(color.r);
    w.put_u8(color.g);
    w.put_u8(color.b);
    w.put_string_utf8(name);
    w.put_string_utf8(message);
    w
}

/// Build a ServerStat packet (0xFE).
pub fn build_server_stat(json: &str) -> BinaryWriter {
    let mut w = BinaryWriter::new();
    w.put_u8(0xFE);
    w.put_string_utf8(json);
    w
}

/// Build a LeaderboardFFA packet (0x31).
pub fn build_leaderboard_ffa(entries: &[(bool, &str)]) -> BinaryWriter {
    let mut w = BinaryWriter::new();
    w.put_u8(0x31);
    w.put_u32(entries.len() as u32);
    for (is_me, name) in entries {
        w.put_u32(if *is_me { 1 } else { 0 });
        w.put_string_utf8(name);
    }
    w
}

/// Build a LeaderboardPie packet (0x32) for teams mode.
pub fn build_leaderboard_pie(team_sizes: &[f32]) -> BinaryWriter {
    let mut w = BinaryWriter::new();
    w.put_u8(0x32);
    w.put_u32(team_sizes.len() as u32);
    for size in team_sizes {
        w.put_f32(*size);
    }
    w
}

/// Cell flags for UpdateNodes packet.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellFlags {
    pub is_spiked: bool,
    pub is_player: bool,
    pub has_skin: bool,
    pub has_name: bool,
    pub is_agitated: bool,
    pub is_ejected: bool,
    pub is_food: bool,
}

impl CellFlags {
    /// Encode flags for protocol 6-10.
    pub fn encode_v6(&self) -> u8 {
        let mut flags = 0u8;
        if self.is_spiked {
            flags |= 0x01;
        }
        if self.is_player {
            flags |= 0x02;
        }
        if self.has_skin {
            flags |= 0x04;
        }
        if self.has_name {
            flags |= 0x08;
        }
        if self.is_agitated {
            flags |= 0x10;
        }
        if self.is_ejected {
            flags |= 0x20;
        }
        if self.is_food {
            flags |= 0x80;
        }
        flags
    }

    /// Encode flags for protocol 11+.
    pub fn encode_v11(&self) -> u8 {
        self.encode_v6() // Same encoding
    }
}

/// Cell data for the UpdateNodes packet.
#[derive(Debug, Clone)]
pub struct UpdateCell {
    pub node_id: u32,
    pub x: i32,
    pub y: i32,
    pub size: u16,
    pub color: Color,
    pub flags: CellFlags,
    pub skin: Option<String>,
    pub name: Option<String>,
}

/// Eat record (cell was eaten by another).
#[derive(Debug, Clone, Copy)]
pub struct EatRecord {
    pub eaten_id: u32,
    pub eater_id: u32,
}

/// Build an UpdateNodes packet (0x10) - protocol 6-10.
/// 
/// The packet format is:
/// - opcode 0x10
/// - eat_count: u16
/// - eat records: [eater_id ^ scramble, eaten_id ^ scramble] × eat_count
/// - update cells (no terminator, inline with add cells)
/// - add cells
/// - terminator: 0u32
/// - remove_count: u16
/// - remove IDs
pub fn build_update_nodes(
    protocol: u32,
    scramble_id: u32,
    scramble_x: i32,
    scramble_y: i32,
    add_nodes: &[UpdateCell],
    upd_nodes: &[UpdateCell],
    eat_nodes: &[EatRecord],
    del_node_ids: &[u32],
) -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(256);
    w.put_u8(0x10);

    // Write eat records
    w.put_u16(eat_nodes.len() as u16);
    for eat in eat_nodes {
        w.put_u32(eat.eater_id ^ scramble_id);
        w.put_u32(eat.eaten_id ^ scramble_id);
    }

    if protocol < 11 {
        write_update_nodes_v6(
            &mut w,
            scramble_id,
            scramble_x,
            scramble_y,
            add_nodes,
            upd_nodes,
        );
    } else {
        write_update_nodes_v11(
            &mut w,
            scramble_id,
            scramble_x,
            scramble_y,
            add_nodes,
            upd_nodes,
        );
    }

    // Write remove records
    let remove_count = eat_nodes.len() + del_node_ids.len();
    if protocol < 6 {
        w.put_u32(remove_count as u32);
    } else {
        w.put_u16(remove_count as u16);
    }
    for eat in eat_nodes {
        w.put_u32(eat.eaten_id ^ scramble_id);
    }
    for &id in del_node_ids {
        w.put_u32(id ^ scramble_id);
    }

    w
}

/// Write update/add nodes for protocol 6-10.
fn write_update_nodes_v6(
    w: &mut BinaryWriter,
    scramble_id: u32,
    scramble_x: i32,
    scramble_y: i32,
    add_nodes: &[UpdateCell],
    upd_nodes: &[UpdateCell],
) {
    // Write updates
    for node in upd_nodes {
        w.put_u32(node.node_id ^ scramble_id);
        w.put_i32(node.x + scramble_x);
        w.put_i32(node.y + scramble_y);
        w.put_u16(node.size);

        let flags = node.flags.encode_v6();
        w.put_u8(flags);

        // Color only for player cells
        if flags & 0x02 != 0 {
            w.put_u8(node.color.r);
            w.put_u8(node.color.g);
            w.put_u8(node.color.b);
        }
    }

    // Write adds
    for node in add_nodes {
        w.put_u32(node.node_id ^ scramble_id);
        w.put_i32(node.x + scramble_x);
        w.put_i32(node.y + scramble_y);
        w.put_u16(node.size);

        let mut flags = node.flags;
        flags.is_player = true; // Always include color for new nodes
        flags.has_skin = node.skin.is_some();
        flags.has_name = node.name.is_some();
        let f = flags.encode_v6();
        w.put_u8(f);

        // Color
        if f & 0x02 != 0 {
            w.put_u8(node.color.r);
            w.put_u8(node.color.g);
            w.put_u8(node.color.b);
        }

        // Skin
        if f & 0x04 != 0 {
            if let Some(ref skin) = node.skin {
                w.put_string_utf8(skin);
            }
        }

        // Name
        if f & 0x08 != 0 {
            if let Some(ref name) = node.name {
                w.put_string_utf8(name);
            }
        }
    }

    // Terminator
    w.put_u32(0);
}

/// Write update/add nodes for protocol 11+.
fn write_update_nodes_v11(
    w: &mut BinaryWriter,
    scramble_id: u32,
    scramble_x: i32,
    scramble_y: i32,
    add_nodes: &[UpdateCell],
    upd_nodes: &[UpdateCell],
) {
    // Write updates
    for node in upd_nodes {
        w.put_u32(node.node_id ^ scramble_id);
        w.put_i32(node.x + scramble_x);
        w.put_i32(node.y + scramble_y);
        w.put_u16(node.size);

        let flags = node.flags.encode_v11();
        w.put_u8(flags);

        // Extended flag for food
        if flags & 0x80 != 0 {
            w.put_u8(0x01);
        }

        // Color only for player cells
        if flags & 0x02 != 0 {
            w.put_u8(node.color.r);
            w.put_u8(node.color.g);
            w.put_u8(node.color.b);
        }
    }

    // Write adds
    for node in add_nodes {
        w.put_u32(node.node_id ^ scramble_id);
        w.put_i32(node.x + scramble_x);
        w.put_i32(node.y + scramble_y);
        w.put_u16(node.size);

        let mut flags = node.flags;
        flags.is_player = true; // Always include color for new nodes
        flags.has_skin = node.skin.is_some();
        flags.has_name = node.name.is_some();
        let f = flags.encode_v11();
        w.put_u8(f);

        // Extended flag for food
        if f & 0x80 != 0 {
            w.put_u8(0x01);
        }

        // Color
        if f & 0x02 != 0 {
            w.put_u8(node.color.r);
            w.put_u8(node.color.g);
            w.put_u8(node.color.b);
        }

        // Skin (protocol 11 uses % prefix)
        if f & 0x04 != 0 {
            if let Some(ref skin) = node.skin {
                w.put_string_utf8(&format!("%{}", skin));
            }
        }

        // Name
        if f & 0x08 != 0 {
            if let Some(ref name) = node.name {
                w.put_string_utf8(name);
            }
        }
    }

    // Terminator
    w.put_u32(0);
}

/// Player cell data for XRay packet.
#[derive(Debug, Clone)]
pub struct XrayPlayerCell {
    pub node_id: u32,
    pub x: i32,
    pub y: i32,
    pub size: u16,
    pub color: Color,
    pub name: String,
}

/// Build an XrayData packet (0x50).
/// This packet shows all player cells to operators with XRay mode enabled.
pub fn build_xray_data(
    scramble_id: u32,
    scramble_x: i32,
    scramble_y: i32,
    player_cells: &[XrayPlayerCell],
) -> BinaryWriter {
    let mut w = BinaryWriter::with_capacity(256);
    w.put_u8(0x50); // XRay packet ID

    // Write number of player cells
    w.put_u16(player_cells.len() as u16);

    // Write each player cell data
    for cell in player_cells {
        // Apply scrambling like other packets
        w.put_u32(cell.node_id ^ scramble_id);
        w.put_u32((cell.x + scramble_x) as u32);
        w.put_u32((cell.y + scramble_y) as u32);
        w.put_u16(cell.size);

        // Color (RGB)
        w.put_u8(cell.color.r);
        w.put_u8(cell.color.g);
        w.put_u8(cell.color.b);

        // Player name (UTF-8 string with length prefix)
        w.put_string_utf8(&cell.name);
    }

    w
}
