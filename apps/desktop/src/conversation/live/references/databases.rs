use super::*;
use sailry_protocol::{
    connection::Resource,
    database::{Catalog, Outcome, Profile},
};

impl View {
    pub(super) fn reference_ssh(&self) -> Vec<&sailry_protocol::ssh::Profile> {
        self.node
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.ssh)
            .filter(|profile| {
                if let Some(bound) = self.resource {
                    return bound == Resource::Ssh(profile.id);
                }
                self.binding.project.is_some_and(|project| {
                    profile
                        .sharing
                        .as_ref()
                        .is_some_and(|sharing| sharing.includes(project))
                })
            })
            .collect()
    }

    pub(super) fn reference_databases(&self) -> Vec<&Profile> {
        self.node
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.databases)
            .filter(|profile| {
                if let Some(bound) = self.resource {
                    return bound == Resource::Database(profile.id);
                }
                self.binding.project.is_some_and(|project| {
                    profile
                        .sharing
                        .as_ref()
                        .is_some_and(|sharing| sharing.includes(project))
                })
            })
            .collect()
    }

    pub(super) fn load_database_references(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Page::Database(id, name, database) = self.references.page.clone() else {
            return;
        };
        let Some(profile) = self
            .reference_databases()
            .into_iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            self.references.error = true;
            return;
        };
        let generation = self.references.generation;
        self.references.loading = true;
        let client = self.binding.client.clone();
        let command = Command::BrowseDatabase {
            profile: id,
            expected_revision: profile.revision,
            database: database.clone(),
        };
        let stop = self.stop.child_token();
        self.references.stop = stop.clone();
        let task = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(client.prepare(command)) => Some(result),
            }
        });
        self.references.task = Some(cx.spawn_in(window, async move |view, cx| {
            let Ok(Some(result)) = task.await else { return };
            let _ = view.update_in(cx, |view, window, cx| {
                if view.references.generation != generation || !view.references.open {
                    return;
                }
                view.references.loading = false;
                if !view
                    .reference_databases()
                    .iter()
                    .any(|profile| profile.id == id)
                {
                    view.references.rows.clear();
                    view.references.error = true;
                } else if let Ok(Output::DatabaseOutcome(Outcome::Catalog(catalog))) = result {
                    let mut rows = vec![Item::Current(Reference {
                        target: Target::Database {
                            connection: id,
                            database: database.clone(),
                            table: None,
                        },
                        label: database
                            .as_ref()
                            .map_or_else(|| name.clone(), |database| format!("{name}/{database}")),
                    })];
                    match catalog {
                        Catalog::Databases(names) => {
                            rows.extend(names.into_iter().map(|database| {
                                Item::Page(Page::Database(id, name.clone(), Some(database)))
                            }))
                        }
                        Catalog::Tables { database, tables } => {
                            rows.extend(tables.into_iter().map(|table| {
                                Item::Reference(Reference {
                                    label: format!("{database}/{}", table.name),
                                    target: Target::Database {
                                        connection: id,
                                        database: Some(database.clone()),
                                        table: Some(table),
                                    },
                                })
                            }))
                        }
                    }
                    if view.references.page == view.reference_root() {
                        rows.push(Item::Attachment);
                    }
                    view.references.rows = rows;
                } else {
                    view.references.error = true;
                }
                view.reference_rows(window, cx);
                cx.notify();
            });
        }));
    }
}
