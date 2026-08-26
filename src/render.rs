use std::fmt::Write as _;

use crate::visitor::AcceptedGame;

/// Renders one accepted, truncated game as PGN text (tags + movetext),
/// followed by a blank line, ready to be appended to the output stream.
pub fn render_game(game: &AcceptedGame) -> Vec<u8> {
    let mut out = String::with_capacity(256);

    for (tag, value) in &game.tags {
        let _ = writeln!(out, "[{tag} \"{}\"]", escape(value));
    }
    out.push('\n');

    let result = game
        .tags
        .iter()
        .find(|(t, _)| t == "Result")
        .map(|(_, v)| v.as_str())
        .unwrap_or("*");

    let mut col = 0usize;
    for (i, san) in game.sans.iter().enumerate() {
        let token = if i % 2 == 0 {
            format!("{}. {}", i / 2 + 1, san)
        } else {
            san.to_string()
        };
        if col > 0 && col + token.len() + 1 > 79 {
            out.push('\n');
            col = 0;
        } else if col > 0 {
            out.push(' ');
            col += 1;
        }
        out.push_str(&token);
        col += token.len();
    }
    if col > 0 {
        out.push(' ');
    }
    out.push_str(result);
    out.push_str("\n\n");

    out.into_bytes()
}

/// Escapes `"` and `\` per the PGN tag-pair escaping rules.
fn escape(s: &str) -> String {
    if s.contains(['"', '\\']) {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    } else {
        s.to_string()
    }
}
