use vibemud_core::{GameSnapshot, HudStateDto, StatusLineDto};

pub fn render_one_line(dto: &StatusLineDto) -> String {
    format!(
        "[VibeMUD] Lv.{} {} | HP {}/{} | {} | {} | Party {}/4 | Danger: {} | Loot {}",
        dto.level,
        dto.class_label,
        dto.hp,
        dto.max_hp,
        dto.area_label,
        dto.mode_label,
        dto.party_count,
        dto.danger_label,
        dto.loot_count
    )
}

pub fn render_compact(dto: &StatusLineDto) -> String {
    format!(
        "Lv{} {} HP{}/{} {} {} P{}/4 D:{}",
        dto.level,
        short_class(&dto.class_label),
        dto.hp,
        dto.max_hp,
        dto.area_label.replace(' ', ""),
        dto.mode_label.replace(' ', ""),
        dto.party_count,
        dto.danger_label
    )
}

pub fn render_ultra_compact(dto: &StatusLineDto) -> String {
    format!(
        "L{} {} HP{} D:{} P{}",
        dto.level,
        short_class(&dto.class_label),
        dto.hp,
        dto.danger_label.chars().next().unwrap_or('S'),
        dto.party_count
    )
}

pub fn render_for_width(dto: &StatusLineDto, width: usize) -> String {
    if width < 60 {
        render_ultra_compact(dto)
    } else if width < 100 {
        render_compact(dto)
    } else {
        render_one_line(dto)
    }
}

pub fn render_side_panel(dto: &StatusLineDto, unicode: bool) -> String {
    if unicode {
        format!(
            "┌──── VibeMUD ────┐\n│ Lv.{:<12}│\n│ {:<16}│\n│ HP {:>4}/{:<7}│\n│ {:<16}│\n│ {:<16}│\n│ Party {:<9}│\n│ Danger {:<8}│\n└─────────────────┘",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            truncate(&dto.area_label, 16),
            truncate(&dto.mode_label, 16),
            format!("{}/4", dto.party_count),
            dto.danger_label
        )
    } else {
        format!(
            "+---- VibeMUD ----+\n| Lv.{:<12}|\n| {:<16}|\n| HP {:>4}/{:<7}|\n| {:<16}|\n| {:<16}|\n| Party {:<9}|\n| Danger {:<8}|\n+-----------------+",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            truncate(&dto.area_label, 16),
            truncate(&dto.mode_label, 16),
            format!("{}/4", dto.party_count),
            dto.danger_label
        )
    }
}

pub fn render_full(snapshot: &GameSnapshot) -> String {
    let dto = StatusLineDto::from(snapshot);
    format!(
        "VibeMUD Character Status\nName: {}\nClass: {}\nLevel: {}\nHP: {}/{}\nMP: {}/{}\nATK: {} DEF: {} ACC: {} RES: {} SPD: {} LUCK: {}\nXP: {}/{} Gold: {}\nArea: {}\nMode: {}\nParty: {}/4\nDanger: {}\nRecent log:\n{}",
        snapshot.player.name,
        dto.class_label,
        snapshot.player.level,
        snapshot.player.hp,
        snapshot.player.max_hp,
        snapshot.player.mp,
        snapshot.player.max_mp,
        snapshot.player.attack,
        snapshot.player.defense,
        snapshot.player.accuracy,
        snapshot.player.evasion,
        snapshot.player.speed,
        snapshot.player.luck,
        snapshot.player.xp,
        snapshot.player.xp_to_next,
        snapshot.player.gold,
        dto.area_label,
        dto.mode_label,
        dto.party_count,
        dto.danger_label,
        snapshot.recent_log.iter().rev().take(10).cloned().collect::<Vec<_>>().join("\n")
    )
}

pub fn hud_state_from_snapshot(snapshot: &GameSnapshot) -> HudStateDto {
    let dto = StatusLineDto::from(snapshot);
    let compact_json = serde_json_like(&render_compact(&dto));
    let full_json = serde_json_like(&render_full(snapshot));
    HudStateDto {
        state_version: snapshot.state_version,
        one_line: render_one_line(&dto),
        compact_json,
        full_json,
    }
}

fn serde_json_like(value: &str) -> String {
    format!("{{\"text\":{}}}", quote(value))
}

fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn short_class(value: &str) -> &str {
    match value {
        "Warrior" => "WAR",
        "전사" => "전",
        "Rogue" => "ROG",
        "도적" => "도",
        "Mage" => "MAG",
        "마법사" => "마",
        "Healer" => "HEL",
        "치유사" => "치",
        _ => "ADV",
    }
}

fn truncate(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibemud_core::GameSnapshot;

    #[test]
    fn width_fallbacks() {
        let snapshot = GameSnapshot::initial();
        let dto = StatusLineDto::from(&snapshot);
        assert!(render_for_width(&dto, 40).starts_with('L'));
        assert!(render_for_width(&dto, 80).starts_with("Lv"));
        assert!(render_for_width(&dto, 120).starts_with("[VibeMUD]"));
    }

    #[test]
    fn ascii_side_panel() {
        let snapshot = GameSnapshot::initial();
        let dto = StatusLineDto::from(&snapshot);
        assert!(render_side_panel(&dto, false).contains("+----"));
    }
}
