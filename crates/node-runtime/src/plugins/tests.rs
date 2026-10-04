use super::*;
use sailry_protocol::plugin::IssueKind;
use std::fs;

fn manifest(name: &str) -> String {
    serde_json::json!({"$schema": manifest::SCHEMA, "name": name}).to_string()
}

pub(super) fn fixture() -> (tempfile::TempDir, Host, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::create_dir(base.join("node")).unwrap();
    let package = base.join("source");
    fs::create_dir_all(package.join("skills/analysis/references")).unwrap();
    fs::create_dir_all(package.join(sailry_protocol::plugin::NAMESPACE)).unwrap();
    fs::write(package.join("plugin.json"), manifest("example")).unwrap();
    fs::write(
        package.join("skills/analysis/SKILL.md"),
        "---\nname: analysis\ndescription: Analyze project data\n---\nRead references/guide.md\n",
    )
    .unwrap();
    fs::write(
        package.join("skills/analysis/references/guide.md"),
        "保留全部资源 🙂\n",
    )
    .unwrap();
    (temp, Host::new(Some(base.join("node"))), package)
}

mod manifest_loading {
    use super::*;

    #[test]
    fn validates_catalog_metadata() {
        for (icon, valid) in [
            ("dev.sailry.platform/icon.png", true),
            ("dev.sailry.platform/assets/icon.png", true),
            ("icon.png", false),
            ("folder", false),
            ("../private.svg", false),
            ("https://example.com/icon.svg", false),
        ] {
            let mut value: serde_json::Value = serde_json::from_str(&manifest("example")).unwrap();
            value["extensions"] = serde_json::json!({"dev.sailry.platform": {
                "api_version":"v1", "actions":[], "icon":icon,
                "description":{"label":"Browse files","locales":{"zh-CN":"浏览文件"}}
            }});
            let parsed = manifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(parsed.extension.is_some(), valid);
        }
    }

    #[test]
    fn tool_handlers_require_declared_exports() {
        let original: serde_json::Value =
            serde_json::from_str(include_str!("../../../../plugins/goals/plugin.json")).unwrap();
        assert!(
            manifest::parse(original.to_string().as_bytes())
                .unwrap()
                .extension
                .is_some()
        );
        for mutation in 0..5 {
            let mut value = original.clone();
            let extension = &mut value["extensions"]["dev.sailry.platform"];
            match mutation {
                0 => extension["host"] = serde_json::Value::Null,
                1 => extension["host"]["handlers"] = serde_json::json!(["unknown"]),
                2 => extension["tools"][0]["operation"] = serde_json::json!("settings.read"),
                3 => {
                    extension["tools"][0]["handler"]["parameters"] =
                        serde_json::json!({"type":"string"})
                }
                4 => extension["tools"][0]["handler"]["name"] = serde_json::json!("../escape"),
                _ => unreachable!(),
            }
            assert!(
                manifest::parse(value.to_string().as_bytes())
                    .unwrap()
                    .extension
                    .is_none()
            );
        }
    }

    #[test]
    fn operation_handlers_require_actions_and_result_exports() {
        let original: serde_json::Value =
            serde_json::from_str(include_str!("../../../../plugins/commands/plugin.json")).unwrap();
        assert!(
            manifest::parse(original.to_string().as_bytes())
                .unwrap()
                .extension
                .is_some()
        );
        for mutation in 0..6 {
            let mut value = original.clone();
            let extension = &mut value["extensions"]["dev.sailry.platform"];
            match mutation {
                0 => extension["actions"] = serde_json::json!(["process.read"]),
                1 => extension["tools"][0]["handler"]["result"] = serde_json::json!("missing"),
                2 => extension["tools"][0]["handler"]["read_only"] = serde_json::json!("true"),
                3 => extension["tools"][0]["handler"]["operation"] = serde_json::Value::Null,
                4 => {
                    extension["tools"][0]["handler"]["operation"] =
                        serde_json::json!("progress.update")
                }
                5 => {
                    extension["tools"][0]["handler"]["operation"] =
                        serde_json::json!("settings.read")
                }
                _ => unreachable!(),
            }
            assert!(
                manifest::parse(value.to_string().as_bytes())
                    .unwrap()
                    .extension
                    .is_none(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn scripts_require_an_entry() {
        let mut value: serde_json::Value = serde_json::from_str(&manifest("example")).unwrap();
        value["extensions"] = serde_json::json!({"dev.sailry.platform": {
            "api_version":"v1", "actions":[],
            "desktop":{"resources":[], "navigation":{"label":"Example","icon":"reicon:files/file-text"}}
        }});
        let parsed = manifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(parsed.extension.is_none());
        assert!(
            parsed
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::InvalidExtensions)
        );
    }

    #[test]
    fn rejects_invalid_fields() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"$schema": manifest::SCHEMA, "name": "Bad"}),
            serde_json::json!({"$schema": manifest::SCHEMA, "name": "valid", "version": null}),
            serde_json::json!({"$schema": manifest::SCHEMA, "name": "valid", "author": {"unknown": "x"}}),
        ] {
            assert!(manifest::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        for name in ["", "bad--name", "bad..name", "-start", "end.", "path/name"] {
            assert!(manifest::parse(manifest(name).as_bytes()).is_err());
        }
    }

    #[test]
    fn ignores_foreign_fields() {
        let mut value: serde_json::Value = serde_json::from_str(&manifest("acme.tools")).unwrap();
        value["skills"] = serde_json::json!(["outside"]);
        value["extensions"] = serde_json::json!({"org.example.other": false});
        value["version"] = serde_json::json!("rolling build");
        value["homepage"] = serde_json::json!("not a URL");
        let parsed = manifest::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(parsed.name, "acme.tools");
        assert_eq!(parsed.issues.len(), 1);
        assert_eq!(parsed.issues[0].kind, IssueKind::IgnoredManifestField);
        value["extensions"] = serde_json::json!(false);
        assert!(
            manifest::parse(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .issues
                .iter()
                .any(|issue| issue.kind == IssueKind::InvalidExtensions)
        );
    }
}

mod extension_loading {
    use super::*;
    use sailry_protocol::plugin::Action;

    #[test]
    fn confines_tool_presentation() {
        use sailry_protocol::tool::Presentation;
        let info = install_extension(
            serde_json::json!({"api_version":"v1","actions":[],"tools":[{"server":"native","name":"read","presentation":"summary","grouping":"standalone"},{"server":"native","name":"write","presentation":"details"},{"server":"a/b","name":"c","presentation":"summary"},{"server":"a","name":"b/c","presentation":"details"}]}),
        );
        assert_eq!(
            info.extension.as_ref().unwrap().tools[0].presentation,
            Presentation::Summary
        );
        let resources = resources::Resources::new(
            Host::new(None),
            vec![info],
            Default::default(),
            sailry_link::CancellationToken::new(),
        );
        assert_eq!(
            resources.tool_presentation("example", "native", "read"),
            Presentation::Summary
        );
        assert_eq!(
            resources.tool_presentation("example", "a/b", "c"),
            Presentation::Summary
        );
        assert_eq!(
            resources.tool_presentation("example", "a", "b/c"),
            Presentation::Details
        );
        assert_eq!(
            resources.tool_grouping("example", "native", "read"),
            sailry_protocol::tool::Grouping::Standalone
        );
        assert_eq!(
            resources.tool_grouping("other", "native", "read"),
            sailry_protocol::tool::Grouping::Sequence
        );
        for (package, server, tool) in [
            ("other", "native", "read"),
            ("example", "other", "read"),
            ("example", "native", "write"),
        ] {
            assert_eq!(
                resources.tool_presentation(package, server, tool),
                Presentation::Details
            );
        }
        for tools in [
            serde_json::json!([{"server":"native","name":"read","presentation":true}]),
            serde_json::json!([{"name":"","presentation":"summary"}]),
            serde_json::json!([{"name":"read"},{"name":"read","presentation":"summary"}]),
            serde_json::json!([{"name":"save", "operation":"storage.set"}]),
            serde_json::json!([{"name":"progress", "operation":"progress.update", "presentation":"details"}]),
            serde_json::json!([{"server":"native", "name":"read", "operation":"storage.get"}]),
        ] {
            let info = install_extension(
                serde_json::json!({"api_version":"v1","actions":[],"tools":tools}),
            );
            assert!(info.extension.is_none());
        }
    }

    fn install_extension(value: serde_json::Value) -> Info {
        let (_temp, host, package) = fixture();
        let mut manifest: serde_json::Value = serde_json::from_str(&manifest("example")).unwrap();
        manifest["extensions"] = serde_json::json!({"dev.sailry.platform": value});
        fs::write(package.join("plugin.json"), manifest.to_string()).unwrap();
        host.install(&package, "", "example").unwrap()
    }

    #[test]
    fn captures_declared_actions() {
        let info = install_extension(
            serde_json::json!({"api_version":"v1", "actions":["files.read", "files.write", "git.read", "conversation.read"]}),
        );
        let extension = info.extension.unwrap();
        assert_eq!(extension.api_version, "v1");
        assert_eq!(
            extension.actions,
            [
                Action::ReadFiles,
                Action::WriteFiles,
                Action::ReadGit,
                Action::ReadConversation
            ]
        );
        assert_eq!(info.skills.len(), 1);
        assert!(info.issues.is_empty());
    }

    #[test]
    fn ignores_metadata_without_changing_declared_actions() {
        let info = install_extension(serde_json::json!({
            "api_version":"v1", "actions":["files.read"], "unknown":true,
            "tools":[{"name":"read", "presentation":"hidden", "unknown":true}],
            "desktop":{"entry":"dev.sailry.platform/ui/main.js",
                "resources":["dev.sailry.platform/ui/main.js"], "network":true}
        }));
        assert!(info.issues.is_empty());
        let extension = info.extension.unwrap();
        assert_eq!(extension.actions, [Action::ReadFiles]);
        assert_eq!(
            extension.tools[0].presentation,
            sailry_protocol::tool::Presentation::Details
        );
        assert!(extension.desktop.unwrap().valid());
        let implicit = install_extension(serde_json::json!({"api_version":"v1"}));
        assert!(implicit.issues.is_empty());
        assert!(implicit.extension.unwrap().actions.is_empty());
    }

    #[test]
    fn captures_plugin_scope() {
        for (scope, expected) in [
            (None, sailry_protocol::plugin::Scope::Host),
            (Some("host"), sailry_protocol::plugin::Scope::Host),
            (Some("desktop"), sailry_protocol::plugin::Scope::Desktop),
        ] {
            let mut value = serde_json::json!({"api_version":"v1", "actions":[]});
            if let Some(scope) = scope {
                value["scope"] = scope.into();
            }
            let info = install_extension(value);
            assert!(info.issues.is_empty());
            assert_eq!(info.extension.unwrap().scope, expected);
        }
        let info = install_extension(
            serde_json::json!({"api_version":"v1", "scope":"global", "actions":[]}),
        );
        assert!(info.extension.is_none());
        assert!(
            info.issues
                .iter()
                .any(|issue| issue.kind == IssueKind::InvalidExtensions)
        );
    }

    #[test]
    fn isolates_invalid_declarations() {
        for value in [
            serde_json::json!({"api_version":"v1", "actions":["files.read", "files.read"]}),
            serde_json::json!({"api_version":"v1", "actions":["process.execute"]}),
            serde_json::json!({"api_version":"v1", "actions":true}),
            serde_json::json!({"api_version":"v1", "actions":[], "display":{"label":""}}),
            serde_json::json!({"api_version":true}),
            serde_json::json!(false),
        ] {
            let info = install_extension(value);
            assert!(info.extension.is_none());
            assert_eq!(info.skills.len(), 1);
            assert_eq!(info.issues[0].kind, IssueKind::InvalidExtensions);
        }
        let info = install_extension(serde_json::json!({"api_version":"v99", "actions":[]}));
        assert!(info.extension.is_none());
        assert_eq!(info.issues[0].kind, IssueKind::UnsupportedExtension);
    }

    #[test]
    fn validates_desktop_resources() {
        let info = install_extension(serde_json::json!({
            "api_version":"v1", "actions":[],
            "desktop":{"entry":"dev.sailry.platform/ui/main.js", "resources":["dev.sailry.platform/ui/main.js", "dev.sailry.platform/ui/locales/en.json"]}
        }));
        assert!(info.extension.unwrap().desktop.unwrap().valid());
        assert_eq!(info.skills.len(), 1);
        for desktop in [
            serde_json::json!({"entry":"../main.js", "resources":["../main.js"]}),
            serde_json::json!({"entry":"dev.sailry.platform/ui/main.js", "resources":[]}),
            serde_json::json!({"entry":"dev.sailry.platform/ui/main.js", "resources":["dev.sailry.platform/ui/main.js", "dev.sailry.platform/ui/main.js"]}),
            serde_json::json!({"entry":"dev.sailry.platform/ui/main.js", "resources":["dev.sailry.platform/ui/main.js"], "ui_entry":"../private.js"}),
        ] {
            let info = install_extension(
                serde_json::json!({"api_version":"v1", "actions":[], "desktop":desktop}),
            );
            assert!(info.extension.is_none());
            assert_eq!(info.skills.len(), 1);
            assert_eq!(info.issues[0].kind, IssueKind::InvalidExtensions);
        }
    }

    #[test]
    fn validates_ui_contributions() {
        use serde_json::json;
        let declaration = json!({"id":"focus", "slot":"composer", "kind":"toggle",
            "label":{"label":"Focus"}, "handler":"focus"});
        let extension = json!({"api_version":"v1", "actions":[], "ui":[declaration],
            "desktop":{"entry":"dev.sailry.platform/ui/main.js", "ui_entry":"dev.sailry.platform/ui/controls.js",
                "resources":["dev.sailry.platform/ui/main.js", "dev.sailry.platform/ui/controls.js"]}});
        assert!(install_extension(extension.clone()).extension.is_some());
        for path in [
            "missing_entry",
            "undeclared_entry",
            "duplicate",
            "metric_handler",
            "icon_path",
        ] {
            let mut invalid = extension.clone();
            match path {
                "missing_entry" => invalid["desktop"]
                    .as_object_mut()
                    .unwrap()
                    .remove("ui_entry")
                    .map(|_| ())
                    .unwrap(),
                "undeclared_entry" => {
                    invalid["desktop"]["ui_entry"] = json!("dev.sailry.platform/ui/missing.js")
                }
                "duplicate" => invalid["ui"]
                    .as_array_mut()
                    .unwrap()
                    .push(declaration.clone()),
                "metric_handler" => {
                    invalid["ui"][0]["kind"] = json!("metric");
                    invalid["ui"][0]["slot"] = json!("statistics");
                }
                "icon_path" => invalid["ui"][0]["icon"] = json!("../private.svg"),
                _ => unreachable!(),
            }
            let info = install_extension(invalid);
            assert!(info.extension.is_none(), "accepted {path}");
            assert_eq!(info.skills.len(), 1);
            assert_eq!(info.issues[0].kind, IssueKind::InvalidExtensions);
        }
    }
}

mod skill_loading {
    use super::*;

    #[test]
    fn reads_display_names_without_changing_skill_ids() {
        let (_temp, host, package) = fixture();
        let skill = package.join("skills/analysis");
        fs::write(skill.join("SKILL.md"), "---\nname: analysis\ndescription: Analyze project data\nmetadata:\n  display-name: Budget Analysis\n---\nInstructions\n").unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert_eq!(info.skills[0].name, "analysis");
        assert_eq!(
            info.skills[0].display_name.as_deref(),
            Some("Budget Analysis")
        );
        fs::create_dir(skill.join("agents")).unwrap();
        fs::write(
            skill.join("agents/openai.yaml"),
            "interface:\n  display_name: Three-Statement Forecast\n",
        )
        .unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert_eq!(info.skills[0].name, "analysis");
        assert_eq!(
            info.skills[0].display_name.as_deref(),
            Some("Three-Statement Forecast")
        );
        fs::write(skill.join("agents/openai.yaml"), "interface: invalid").unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert_eq!(
            info.skills[0].display_name.as_deref(),
            Some("Budget Analysis")
        );
    }

    #[test]
    fn parses_unprivileged_skills() {
        let text = "---\r\nname: analysis\r\ndescription: >\r\n  Analyze data\r\n  and report findings\r\nallowed-tools: Bash(git:*) Read\r\nmetadata:\r\n  version: '1'\r\nlicense: MIT\r\ncompatibility: Rust\r\n---\r\n保留正文 🙂\r\n";
        let skill = skills::parse("analysis", text).unwrap();
        assert_eq!(skill.description, "Analyze data and report findings\n");
        assert_eq!(skill.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn accepts_tool_lists() {
        for tools in [
            "[]",
            "[Read]",
            "[Bash(curl:*), Bash(jq:*), Read]",
            "\n  - Bash(curl:*)\n  - Bash(jq:*)\n  - Read",
        ] {
            let text = format!(
                "---\nname: context7\ndescription: Retrieve library documentation\nallowed-tools: {tools}\n---\nInstructions\n"
            );
            let skill = skills::parse("context7", &text).unwrap();
            assert_eq!(skill.name, "context7");
            assert_eq!(skill.description, "Retrieve library documentation");
        }
    }

    #[test]
    fn installs_tool_lists() {
        let (_temp, host, package) = fixture();
        fs::write(package.join("skills/analysis/SKILL.md"), "---\nname: analysis\ndescription: Analyze project data\nallowed-tools:\n  - Bash(curl:*)\n  - Bash(jq:*)\n  - Read\n---\nInstructions\n").unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert!(info.issues.is_empty());
        assert_eq!(info.skills.len(), 1);
        assert_eq!(info.skills[0].name, "analysis");
        assert!(info.extension.is_none());
        assert!(info.mcp.is_empty());
    }

    #[test]
    fn rejects_invalid_frontmatter() {
        for header in [
            "name: other\ndescription: text",
            "name: analysis\ndescription: ''",
            "name: analysis\nname: analysis\ndescription: text",
            "name: analysis\ndescription: text\nallowed-tools: [Read, 1]",
            "name: analysis\ndescription: text\nallowed-tools: [[Read]]",
            "name: analysis\ndescription: text\nallowed-tools: {Read: true}",
            "name: analysis\ndescription: text\nallowed-tools: false",
            "name: analysis\ndescription: text\nallowed-tools: null",
            "name: analysis\ndescription: text\nmetadata: {version: 1}",
            "name: analysis\ndescription: text\ncompatibility: ''",
        ] {
            assert!(
                skills::parse("analysis", &format!("---\n{header}\n---\nBody")).is_err(),
                "accepted {header}"
            );
        }
        assert!(skills::parse("analysis", "---\nname: analysis\ndescription: text").is_err());
    }

    #[test]
    fn discovers_direct_skills() {
        let (_temp, _host, package) = fixture();
        fs::create_dir_all(package.join("skills/invalid")).unwrap();
        fs::write(package.join("skills/invalid/SKILL.md"), "not a skill").unwrap();
        fs::create_dir_all(package.join("skills/group/nested")).unwrap();
        fs::write(
            package.join("skills/group/nested/SKILL.md"),
            "---\nname: nested\ndescription: text\n---\nBody",
        )
        .unwrap();
        fs::write(package.join("mcp.json"), "{}").unwrap();
        let root = crate::files::path::root(&package).unwrap();
        let info = describe(
            &root,
            manifest::parse(manifest("example").as_bytes()).unwrap(),
            String::new(),
            Vec::new(),
        )
        .unwrap();
        assert_eq!(info.skills.len(), 1);
        assert_eq!(info.skills[0].name, "analysis");
        assert!(
            info.issues
                .iter()
                .any(|issue| issue.kind == IssueKind::InvalidSkill)
        );
        assert!(
            info.issues
                .iter()
                .any(|issue| issue.kind == IssueKind::InvalidMcp)
        );
    }

    #[test]
    fn rejects_invalid_installation() {
        for invalid_directory in [false, true] {
            let (_temp, host, package) = fixture();
            if invalid_directory {
                fs::remove_dir_all(package.join("skills")).unwrap();
                fs::write(package.join("skills"), "not a directory").unwrap();
            } else {
                fs::create_dir_all(package.join("skills/invalid")).unwrap();
                fs::write(package.join("skills/invalid/SKILL.md"), "not a skill").unwrap();
            }
            let error = host.install(&package, "", "example").unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest);
            assert_eq!(error.message, "plugin contains invalid skills");
            assert_eq!(
                fs::read_dir(host.profile.as_ref().unwrap().join("plugins/packages"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}

mod package_storage {
    use super::*;

    #[test]
    fn retains_immutable_revisions() {
        let (_temp, host, package) = fixture();
        let first = host.install(&package, "", "example").unwrap();
        let repeated = host.install(&package, "", "example").unwrap();
        assert_eq!(first, repeated);
        let stored = host
            .profile
            .as_ref()
            .unwrap()
            .join("plugins/packages")
            .join(&first.summary.digest);
        assert_eq!(
            fs::read(stored.join("skills/analysis/references/guide.md")).unwrap(),
            "保留全部资源 🙂\n".as_bytes()
        );
        fs::write(
            package.join("skills/analysis/references/guide.md"),
            "New contents",
        )
        .unwrap();
        let updated = host.install(&package, "", "example").unwrap();
        assert_ne!(first.summary.digest, updated.summary.digest);
        assert_eq!(
            fs::read(stored.join("skills/analysis/references/guide.md")).unwrap(),
            "保留全部资源 🙂\n".as_bytes()
        );
    }

    #[test]
    fn rejects_changed_objects() {
        let (_temp, host, package) = fixture();
        let first = host.install(&package, "", "example").unwrap();
        let stored = host
            .profile
            .as_ref()
            .unwrap()
            .join("plugins/packages")
            .join(first.summary.digest)
            .join("skills/analysis/SKILL.md");
        fs::write(&stored, "External change").unwrap();
        assert_eq!(
            host.install(&package, "", "example").unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert_eq!(fs::read_to_string(stored).unwrap(), "External change");
    }

    #[test]
    fn validates_package_identity() {
        let (_temp, host, package) = fixture();
        assert!(host.install(&package, "../node", "example").is_err());
        assert!(host.install(&package, "", "other").is_err());
        assert_eq!(
            host.install(host.profile.as_ref().unwrap(), "", "example")
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        assert!(!host.profile.as_ref().unwrap().join("plugins").exists());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_confined_executables() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (temp, host, package) = fixture();
        let outside = temp.path().join("outside");
        fs::write(&outside, "Do not import").unwrap();
        symlink(&outside, package.join("secret")).unwrap();
        let script = package.join("run.sh");
        fs::write(&script, "#!/bin/sh\nexit 23\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let info = host.install(&package, "", "example").unwrap();
        assert_eq!(info.skills.len(), 1);
        assert!(info.issues.iter().any(|issue| issue.path == "secret"));
        let stored = host
            .profile
            .as_ref()
            .unwrap()
            .join("plugins/packages")
            .join(info.summary.digest);
        assert!(!stored.join("secret").exists());
        assert_ne!(
            fs::metadata(stored.join("run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        fs::remove_file(package.join("plugin.json")).unwrap();
        symlink(&outside, package.join("plugin.json")).unwrap();
        assert!(host.install(&package, "", "example").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_replaced_storage() {
        use std::os::unix::fs::symlink;
        let (temp, host, package) = fixture();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, host.profile.as_ref().unwrap().join("plugins")).unwrap();
        assert!(host.install(&package, "", "example").is_err());
        assert!(fs::read_dir(outside).unwrap().next().is_none());
    }

    #[test]
    fn bounds_package_and_skill_resources() {
        let (_temp, host, package) = fixture();
        let file = fs::File::create(package.join("large.bin")).unwrap();
        file.set_len(128 * 1024 * 1024 + 1).unwrap();
        assert!(host.install(&package, "", "example").is_err());
        fs::remove_file(package.join("large.bin")).unwrap();
        fs::write(
            package.join("skills/analysis/SKILL.md"),
            "x".repeat(skills::MAX_BYTES + 1),
        )
        .unwrap();
        assert_eq!(
            host.install(&package, "", "example").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}
