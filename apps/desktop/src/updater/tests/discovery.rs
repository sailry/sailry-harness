use super::*;
use crate::updater::{
    config::Channel,
    tests::{fixture, transfer::server},
};
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn release(base: &str, version: &str) -> Value {
    let mut assets = vec!["update-v1.json".to_owned()];
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assets.extend([
            format!("Sailry-{target}.dmg"),
            format!("Sailry-{target}.zip"),
            format!("Sailry-{target}.zip.update.json"),
            format!("SHA256SUMS-desktop-{target}"),
        ]);
    }
    json!({"tag_name":format!("v{version}"), "draft":false,
        "prerelease": !semver::Version::parse(version).unwrap().pre.is_empty(),
        "assets":assets.into_iter().map(|name| json!({"state":"uploaded","size":512,
            "browser_download_url":format!("{base}/v{version}/{name}"),"name":name})).collect::<Vec<_>>()})
}

fn catalog(base: &str, version: &str) -> Vec<u8> {
    let mut release = fixture::release();
    release.version = version.into();
    release.name = format!("Sailry-{}.zip", release.target);
    release.url = format!("{base}/v{version}/{}", release.name);
    fixture::signed(&[release])
}

fn configure(base: &str) -> Config {
    let mut config = fixture::config();
    config.source = format!("{base}/releases");
    config.downloads = base.into();
    config
}

#[tokio::test]
async fn selects_semver_by_channel_not_api_order() {
    for (channel, expected) in [
        (Channel::Stable, "2.10.0"),
        (Channel::Experience, "3.0.0-alpha.2"),
    ] {
        let (base, serving) = server(|base| {
            BTreeMap::from([
                (
                    "/releases?per_page=100&page=1".into(),
                    serde_json::to_vec(&vec![
                        release(base, "2.9.0"),
                        release(base, "3.0.0-alpha.2"),
                        release(base, "2.10.0"),
                    ])
                    .unwrap(),
                ),
                (
                    format!("/v{expected}/update-v1.json"),
                    catalog(base, expected),
                ),
            ])
        })
        .await;
        let mut config = configure(&base);
        config.channel = channel;
        let selection = transfer::check(
            transfer::client().unwrap(),
            &config,
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selection.release.version, expected);
        assert!(
            selection
                .release
                .url
                .starts_with(&format!("{base}/v{expected}/"))
        );
        serving.await.unwrap();
    }
}

#[tokio::test]
async fn ordinary_previews_keep_semver_channels() {
    for (channel, expected) in [
        (Channel::Stable, "2.0.0"),
        (Channel::Experience, "3.0.0-alpha.3"),
    ] {
        let (base, serving) = server(|base| {
            let mut preview = release(base, "3.0.0-alpha.3");
            preview["prerelease"] = false.into();
            BTreeMap::from([
                (
                    "/releases?per_page=100&page=1".into(),
                    serde_json::to_vec(&vec![preview, release(base, "2.0.0")]).unwrap(),
                ),
                (
                    format!("/v{expected}/update-v1.json"),
                    catalog(base, expected),
                ),
            ])
        })
        .await;
        let mut config = configure(&base);
        config.channel = channel;
        let selected = transfer::check(
            transfer::client().unwrap(),
            &config,
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(selected.release.version, expected);
        assert!(
            selected
                .release
                .url
                .starts_with(&format!("{base}/v{expected}/"))
        );
        serving.await.unwrap();
    }
}

#[test]
fn stable_versions_reject_prerelease_flags() {
    let base = "https://github.com/sailry/sailry-harness/releases/download";
    let mut config = configure(base);
    config.channel = Channel::Experience;
    let mut value = release(base, "3.0.0");
    value["prerelease"] = true.into();
    assert!(
        serde_json::from_value::<Release>(value)
            .unwrap()
            .published(&config)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn skips_adhoc_and_requires_exact_host_version() {
    for required in [None, Some("1.0.0".to_owned())] {
        let (base, serving) = server(|base| {
            let mut adhoc = release(base, "1.0.0");
            adhoc["assets"]
                .as_array_mut()
                .unwrap()
                .retain(|asset| asset["name"] != "update-v1.json");
            let mut responses = BTreeMap::from([(
                "/releases?per_page=100&page=1".into(),
                serde_json::to_vec(&vec![adhoc, release(base, "2.0.0")]).unwrap(),
            )]);
            if required.is_none() {
                responses.insert("/v2.0.0/update-v1.json".into(), catalog(base, "2.0.0"));
            }
            responses
        })
        .await;
        let mut config = configure(&base);
        config.required = required.clone();
        let result = transfer::check(
            transfer::client().unwrap(),
            &config,
            &CancellationToken::new(),
        )
        .await;
        if required.is_some() {
            assert_eq!(result.unwrap_err().key, "compatibility_update_unavailable");
        } else {
            assert_eq!(result.unwrap().unwrap().release.version, "2.0.0");
        }
        serving.await.unwrap();
    }
}

#[tokio::test]
async fn follows_pages_before_selecting_highest_version() {
    let (base, serving) = server(|base| {
        let mut draft = release(base, "9.0.0");
        draft["draft"] = true.into();
        BTreeMap::from([
            (
                "/releases?per_page=100&page=1".into(),
                serde_json::to_vec(&vec![draft; 100]).unwrap(),
            ),
            (
                "/releases?per_page=100&page=2".into(),
                serde_json::to_vec(&vec![release(base, "1.0.0")]).unwrap(),
            ),
            ("/v1.0.0/update-v1.json".into(), catalog(base, "1.0.0")),
        ])
    })
    .await;
    let selection = transfer::check(
        transfer::client().unwrap(),
        &configure(&base),
        &CancellationToken::new(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(selection.release.version, "1.0.0");
    serving.await.unwrap();
}

#[test]
fn rejects_mutable_urls_and_incomplete_formal_assets() {
    let base = "https://github.com/sailry/sailry-harness/releases/download";
    let mut config = configure(base);
    config.channel = Channel::Experience;
    let mut value = release(base, "1.0.0");
    for asset in value["assets"].as_array_mut().unwrap() {
        asset["browser_download_url"] = asset["browser_download_url"]
            .as_str()
            .unwrap()
            .replace("/v1.0.0/", "/latest/")
            .into();
    }
    assert!(
        serde_json::from_value::<Release>(value)
            .unwrap()
            .published(&config)
            .is_err()
    );
    for missing in [
        "Sailry-x86_64-apple-darwin.zip.update.json",
        "Sailry-aarch64-apple-darwin.dmg",
    ] {
        let mut value = release(base, "1.0.0");
        value["assets"]
            .as_array_mut()
            .unwrap()
            .retain(|asset| asset["name"] != missing);
        assert!(
            serde_json::from_value::<Release>(value)
                .unwrap()
                .published(&config)
                .unwrap()
                .is_none()
        );
    }
    for invalid in ["size", "state"] {
        let mut value = release(base, "1.0.0");
        value["assets"][0][invalid] = if invalid == "size" {
            0.into()
        } else {
            "starter".into()
        };
        assert!(
            serde_json::from_value::<Release>(value)
                .unwrap()
                .published(&config)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn binds_signed_catalog_to_exact_release() {
    let base = "https://github.com/sailry/sailry-harness/releases/download";
    let config = configure(base);
    let published = serde_json::from_value::<Release>(release(base, "1.0.0"))
        .unwrap()
        .published(&config)
        .unwrap()
        .unwrap();
    published
        .authenticate(&catalog(base, "1.0.0"), &config)
        .unwrap();
    assert!(
        published
            .authenticate(&catalog(base, "2.0.0"), &config)
            .is_err()
    );
    let mut item = fixture::release();
    item.version = "1.0.0".into();
    item.name = format!("Sailry-{}.zip", item.target);
    item.url = format!("{base}/latest/{}", item.name);
    assert!(
        published
            .authenticate(&fixture::signed(&[item]), &config)
            .is_err()
    );
    let mut bytes = catalog(base, "1.0.0");
    let changed = bytes.iter().position(|byte| *byte == b'1').unwrap();
    bytes[changed] = b'2';
    assert!(published.authenticate(&bytes, &config).is_err());
}
