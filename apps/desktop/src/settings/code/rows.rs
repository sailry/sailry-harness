use super::*;

#[derive(Clone)]
pub(super) struct ServerRow {
    pub server: String,
    pub languages: Vec<String>,
    pub candidate: Option<code::Candidate>,
    pub installation: Option<code::Installation>,
}

impl ServerRow {
    pub(super) fn installed(&self) -> bool {
        self.candidate
            .as_ref()
            .is_some_and(|candidate| candidate.status != code::Status::NotInstalled)
            || self
                .installation
                .as_ref()
                .is_some_and(|installation| installation.receipt.is_some())
    }
}

impl Panel {
    pub(super) fn rows(&self) -> Vec<ServerRow> {
        let mut rows = BTreeMap::<String, ServerRow>::new();
        if let Some(catalog) = &self.catalog {
            for language in catalog {
                for candidate in &language.servers {
                    let row =
                        rows.entry(candidate.server.clone())
                            .or_insert_with(|| ServerRow {
                                server: candidate.server.clone(),
                                languages: Vec::new(),
                                candidate: Some(candidate.clone()),
                                installation: self
                                    .installations
                                    .iter()
                                    .find(|installation| {
                                        candidate.package.as_ref().is_some_and(|package| {
                                            package.name == installation.package
                                        })
                                    })
                                    .cloned(),
                            });
                    if !row.languages.contains(&language.name) {
                        row.languages.push(language.name.clone());
                    }
                }
            }
        }
        for installation in &self.installations {
            if rows.values().any(|row| {
                row.installation
                    .as_ref()
                    .is_some_and(|current| current.package == installation.package)
            }) {
                continue;
            }
            rows.insert(
                installation.package.clone(),
                ServerRow {
                    server: installation.package.clone(),
                    languages: Vec::new(),
                    candidate: None,
                    installation: Some(installation.clone()),
                },
            );
        }
        rows.into_values().collect()
    }
}
