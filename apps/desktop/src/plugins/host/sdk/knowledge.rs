//! Knowledge callbacks retain captured sources and exact revision-checked drafts.
use super::*;
use sailry_protocol::knowledge;

impl Host {
    pub(super) fn knowledge_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let query = self.clone();
        let list = self.clone();
        let create = self.clone();
        let rebuild = self.clone();
        let remove = self.clone();
        module
            .async_function("queryKnowledge", move |args| {
                let input: knowledge::Query = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                input.validate().map_err(values::fault)?;
                query.read_public(Command::QueryKnowledge(input))
            })
            .async_function("listKnowledgeCollections", move |_| {
                list.read_public(Command::ListKnowledgeCollections)
            })
            .function("prepareKnowledgeCollection", move |args| {
                let draft: knowledge::Draft = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                create.prepare_public(Command::CreateKnowledgeCollection {
                    definition: draft
                        .into_definition(create.context().worktree)
                        .map_err(values::fault)?,
                    plugin: None,
                })
            })
            .function("prepareKnowledgeRebuild", move |args| {
                let id = args
                    .string(0)?
                    .parse()
                    .map_err(|_| HostError::new("invalid knowledge collection ID"))?;
                let expected_revision = revision(args.string(1)?)?;
                if expected_revision == 0 {
                    return Err(HostError::new(
                        "knowledge collection revision must be positive",
                    ));
                }
                rebuild.prepare_public(Command::RebuildKnowledgeCollection {
                    id,
                    expected_revision,
                })
            })
            .function("prepareKnowledgeRemoval", move |args| {
                let id = args
                    .string(0)?
                    .parse()
                    .map_err(|_| HostError::new("invalid knowledge collection ID"))?;
                let expected_revision = revision(args.string(1)?)?;
                if expected_revision == 0 {
                    return Err(HostError::new(
                        "knowledge collection revision must be positive",
                    ));
                }
                remove.prepare_public(Command::DeleteKnowledgeCollection {
                    id,
                    expected_revision,
                })
            })
    }
}
