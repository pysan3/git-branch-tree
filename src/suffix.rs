//! Configurable per-branch suffix commands for the emitted rebase chain.
//!
//! Every command a branch gets appended after its rebase+push is a template the
//! user controls: `--on-base` for branches landing on the base, `--on-parent` for
//! branches landing on a still-open parent. Placeholders are validated at CLI-parse
//! time so a typo fails before any git work starts.

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Var {
    Branch,
    Onto,
    Base,
    Up,
}

#[derive(Debug, Clone)]
enum Seg {
    Lit(String),
    Var(Var),
}

/// Values available to a template.
pub struct SuffixCtx<'a> {
    pub branch: &'a str,
    pub onto: &'a str,
    pub base: &'a str,
    pub up: &'a str,
}

/// A validated command template: `{branch}`, `{onto}`, `{base}`, `{up}`;
/// `{{` / `}}` escape literal braces.
#[derive(Debug, Clone)]
pub struct SuffixTemplate {
    raw: String,
    segs: Vec<Seg>,
}

impl SuffixTemplate {
    pub fn parse(raw: &str) -> Result<Self> {
        let mut segs = Vec::new();
        let mut lit = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    lit.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    lit.push('}');
                }
                '{' => {
                    let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
                    let var = match name.as_str() {
                        "branch" => Var::Branch,
                        "onto" => Var::Onto,
                        "base" => Var::Base,
                        "up" => Var::Up,
                        other => bail!(
                            "unknown placeholder '{{{other}}}' in command template '{raw}' \
                             (available: {{branch}}, {{onto}}, {{base}}, {{up}})"
                        ),
                    };
                    if !lit.is_empty() {
                        segs.push(Seg::Lit(std::mem::take(&mut lit)));
                    }
                    segs.push(Seg::Var(var));
                }
                '}' => bail!("unmatched '}}' in command template '{raw}'"),
                c => lit.push(c),
            }
        }
        if !lit.is_empty() {
            segs.push(Seg::Lit(lit));
        }
        Ok(Self {
            raw: raw.to_string(),
            segs,
        })
    }

    pub fn expand(&self, ctx: &SuffixCtx<'_>) -> String {
        self.segs
            .iter()
            .map(|seg| match seg {
                Seg::Lit(s) => s.as_str(),
                Seg::Var(Var::Branch) => ctx.branch,
                Seg::Var(Var::Onto) => ctx.onto,
                Seg::Var(Var::Base) => ctx.base,
                Seg::Var(Var::Up) => ctx.up,
            })
            .collect()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }
}

/// Lets clap validate templates while parsing, so a bad placeholder fails before any
/// git work starts rather than after a minute of blame.
impl std::str::FromStr for SuffixTemplate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).map_err(|e| e.to_string())
    }
}

/// The suffix commands for both landing kinds, in append order.
pub struct SuffixConfig {
    pub on_base: Vec<SuffixTemplate>,
    pub on_parent: Vec<SuffixTemplate>,
}

/// Defaults a stack tool wants in place of the crate's own, one side at a time.
///
/// `None` means "keep the crate default", which is not the same as `Some("")` - that
/// would mean "append nothing", and no tool has a reason to say it.
#[derive(Debug, Clone, Copy)]
pub struct SuffixPreset {
    pub on_base: Option<&'static str>,
    pub on_parent: Option<&'static str>,
}

impl SuffixPreset {
    pub const NONE: Self = Self {
        on_base: None,
        on_parent: None,
    };
}

/// The suffix appended to a branch landing on the base: it is ready to ship.
pub const DEFAULT_ON_BASE: &str = "review";
/// The suffix for a branch landing on a parent: a stacked PR whose GitHub base branch
/// must be retargeted to that parent.
pub const DEFAULT_ON_PARENT: &str = "gh pr edit {branch} --base {onto}";

impl SuffixConfig {
    /// Build from CLI values, with a stack tool's preset as the middle layer.
    ///
    /// Three sources, most specific first: the flag the user passed, the preset the
    /// selected tool asks for, then the crate default. Each side is decided on its own,
    /// so `--from-gt-stack --on-base 'x'` still gets gt's `--on-parent`.
    ///
    /// Giving a flag replaces rather than extends, so a user can shrink the chain as
    /// well as grow it; a single empty value emits no suffix at all. An explicit empty
    /// flag therefore beats a preset, which is right - it is the more deliberate of the
    /// two statements.
    pub fn from_cli(
        on_base: Option<&[SuffixTemplate]>,
        on_parent: Option<&[SuffixTemplate]>,
        preset: SuffixPreset,
    ) -> Result<Self> {
        let pick = |vals: Option<&[SuffixTemplate]>,
                    preset: Option<&str>,
                    default: &str|
         -> Result<Vec<SuffixTemplate>> {
            match vals {
                Some(vals) => Ok(vals
                    .iter()
                    .filter(|t| !t.raw().is_empty())
                    .cloned()
                    .collect()),
                None => Ok(vec![SuffixTemplate::parse(preset.unwrap_or(default))?]),
            }
        };
        Ok(Self {
            on_base: pick(on_base, preset.on_base, DEFAULT_ON_BASE)?,
            on_parent: pick(on_parent, preset.on_parent, DEFAULT_ON_PARENT)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SuffixCtx<'static> {
        SuffixCtx {
            branch: "feat/x",
            onto: "feat/parent",
            base: "master",
            up: "0123456789",
        }
    }

    #[test]
    fn expands_placeholders() {
        let t = SuffixTemplate::parse("gh pr edit {branch} --base {onto}").unwrap();
        assert_eq!(t.expand(&ctx()), "gh pr edit feat/x --base feat/parent");
    }

    #[test]
    fn escapes_braces() {
        let t = SuffixTemplate::parse("echo {{literal}} {base}@{up}").unwrap();
        assert_eq!(t.expand(&ctx()), "echo {literal} master@0123456789");
    }

    #[test]
    fn rejects_unknown_placeholder() {
        assert!(SuffixTemplate::parse("echo {nope}").is_err());
        assert!(SuffixTemplate::parse("echo }").is_err());
    }

    #[test]
    fn defaults_and_disabling() {
        let cfg = SuffixConfig::from_cli(None, None, SuffixPreset::NONE).unwrap();
        assert_eq!(cfg.on_base.len(), 1);
        assert_eq!(cfg.on_base[0].raw(), "review");
        assert_eq!(cfg.on_parent[0].raw(), "gh pr edit {branch} --base {onto}");

        let empty = [SuffixTemplate::parse("").unwrap()];
        let cfg = SuffixConfig::from_cli(Some(&empty), Some(&empty), SuffixPreset::NONE).unwrap();
        assert!(cfg.on_base.is_empty());
        assert!(cfg.on_parent.is_empty());
    }

    #[test]
    fn a_given_flag_replaces_the_default_rather_than_adding_to_it() {
        let mine = [
            SuffixTemplate::parse("echo {branch}").unwrap(),
            SuffixTemplate::parse("notify {onto}").unwrap(),
        ];
        let cfg = SuffixConfig::from_cli(Some(&mine), None, SuffixPreset::NONE).unwrap();
        let raws: Vec<&str> = cfg.on_base.iter().map(SuffixTemplate::raw).collect();
        assert_eq!(raws, vec!["echo {branch}", "notify {onto}"]);
        // The other side keeps its default.
        assert_eq!(cfg.on_parent[0].raw(), DEFAULT_ON_PARENT);
    }

    #[test]
    fn a_tool_preset_sits_between_the_flag_and_the_default() {
        let preset = SuffixPreset {
            on_base: Some("gt track --parent {base}"),
            on_parent: Some("gt track --parent {onto}"),
        };

        // Nothing given: the preset replaces both defaults.
        let cfg = SuffixConfig::from_cli(None, None, preset).unwrap();
        assert_eq!(cfg.on_base[0].raw(), "gt track --parent {base}");
        assert_eq!(cfg.on_parent[0].raw(), "gt track --parent {onto}");

        // An explicit flag beats the preset, and only on the side it was given.
        let mine = [SuffixTemplate::parse("echo {branch}").unwrap()];
        let cfg = SuffixConfig::from_cli(Some(&mine), None, preset).unwrap();
        assert_eq!(cfg.on_base[0].raw(), "echo {branch}");
        assert_eq!(cfg.on_parent[0].raw(), "gt track --parent {onto}");

        // Explicitly empty beats it too: it is the more deliberate statement.
        let empty = [SuffixTemplate::parse("").unwrap()];
        let cfg = SuffixConfig::from_cli(None, Some(&empty), preset).unwrap();
        assert_eq!(cfg.on_base[0].raw(), "gt track --parent {base}");
        assert!(cfg.on_parent.is_empty());
    }

    #[test]
    fn parses_from_str_for_clap() {
        assert!("review".parse::<SuffixTemplate>().is_ok());
        let err = "echo {nope}".parse::<SuffixTemplate>().unwrap_err();
        assert!(err.contains("unknown placeholder"), "{err}");
    }
}
