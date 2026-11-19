//! The agent skill: how to be a good member of a workspace. One text serves
//! as a Claude Code skill (`shy skill`) and as the MCP server's instructions.

pub const SKILL: &str = include_str!("../../../skills/shyspace/SKILL.md");

/// The skill without its frontmatter, for places that only want the guidance.
pub fn guidance() -> &'static str {
    SKILL
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(SKILL, |(_, body)| body)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_has_frontmatter_and_guidance() {
        assert!(SKILL.starts_with("---\nname: shyspace\n"));
        assert!(guidance().starts_with("# Working in shyspace"));
    }
}
