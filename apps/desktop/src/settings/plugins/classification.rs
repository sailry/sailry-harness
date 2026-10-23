//! Management follows provenance, including packages with bundled skills or MCP.
use crate::settings::Section;
use sailry_protocol::plugin::{Info, Summary};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::settings) enum Kind {
    Plugin,
    Skill,
    Mcp,
}

impl Kind {
    pub fn of(info: &Info) -> Self {
        if info.skill.is_some() {
            Self::Skill
        } else if info.mcp_source.is_some() {
            Self::Mcp
        } else {
            Self::Plugin
        }
    }

    pub fn section(self) -> Section {
        match self {
            Self::Plugin => Section::Plugins,
            Self::Skill => Section::Skills,
            Self::Mcp => Section::Connectors,
        }
    }
}

pub(in crate::settings) fn inventory<'a>(
    packages: &[Summary],
    entries: &'a BTreeMap<String, Info>,
    kind: Kind,
) -> Vec<&'a Info> {
    packages
        .iter()
        .filter_map(|summary| {
            entries
                .get(&summary.name)
                .filter(|info| info.summary == *summary && Kind::of(info) == kind)
        })
        .collect()
}
