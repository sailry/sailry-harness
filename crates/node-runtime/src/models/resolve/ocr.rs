use super::*;
use sailry_protocol::models::PaddleFamily;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    family: PaddleFamily,
    detector: String,
    recognizer: String,
    dictionary: String,
    orientation: Option<String>,
}

pub(super) async fn profile(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<(String, &'static str)>), Fault> {
    let profile = if let Some(file) = repository.files.get("ocr.json") {
        if file.bytes > super::super::METADATA_BYTES as u64 {
            return Err(Fault::invalid("OCR profile exceeds its metadata limit"));
        }
        let (value, _) = json(http, file, "metadata.json", cancel).await?;
        serde_json::from_value::<Profile>(value)
            .map_err(|_| Fault::invalid("OCR profile is invalid"))?
    } else {
        let known = [
            (
                PaddleFamily::V6,
                "pp-ocrv6_tiny_det.onnx",
                "pp-ocrv6_tiny_rec.onnx",
                "ppocrv6_tiny_dict.txt",
            ),
            (
                PaddleFamily::V5,
                "pp-ocrv5_mobile_det.onnx",
                "pp-ocrv5_mobile_rec.onnx",
                "ppocrv5_dict.txt",
            ),
            (
                PaddleFamily::V6,
                "pp-ocrv6_small_det.onnx",
                "pp-ocrv6_small_rec.onnx",
                "ppocrv6_dict.txt",
            ),
        ];
        let (family, detector, recognizer, dictionary) = known
            .into_iter()
            .find(|(_, detector, recognizer, dictionary)| {
                [detector, recognizer, dictionary]
                    .iter()
                    .all(|path| repository.files.contains_key(**path))
            })
            .ok_or_else(|| Fault::invalid("a supported Paddle OCR profile is required"))?;
        Profile {
            family,
            detector: detector.into(),
            recognizer: recognizer.into(),
            dictionary: dictionary.into(),
            orientation: repository
                .files
                .contains_key("pp-lcnet_x0_25_textline_ori.onnx")
                .then(|| "pp-lcnet_x0_25_textline_ori.onnx".into()),
        }
    };
    let orientation = profile.orientation.is_some();
    let mut files = vec![
        (profile.detector, "detector.onnx"),
        (profile.recognizer, "recognizer.onnx"),
        (profile.dictionary, "dictionary.txt"),
    ];
    if let Some(path) = profile.orientation {
        files.push((path, "orientation.onnx"));
    }
    for (path, name) in &files {
        let file = required(repository, path)?;
        if file.bytes == 0
            || file.bytes
                > if *name == "dictionary.txt" {
                    SMALL_BYTES as u64
                } else {
                    MAX_ASSET_BYTES
                }
            || !super::super::commit(&file.source.revision)
        {
            return Err(Fault::invalid(
                "OCR asset requires a bounded immutable file revision",
            ));
        }
    }
    Ok((
        Format::PaddleOcr {
            family: profile.family,
            orientation,
        },
        files,
    ))
}

pub(super) async fn definition(
    http: &Http,
    repository: &Repository,
    cancel: &CancellationToken,
) -> Result<(Format, Vec<Asset>), Fault> {
    let (format, files) = profile(http, repository, cancel).await?;
    let mut assets = Vec::new();
    for (path, name) in files {
        let file = required(repository, &path)?;
        assets.push(if name == "dictionary.txt" {
            small(http, file, name, cancel).await?.1
        } else {
            asset(http, file, name, cancel).await?
        });
    }
    Ok((format, assets))
}

pub(in crate::models) async fn refresh(
    http: &Http,
    previous: &Definition,
    cancel: &CancellationToken,
) -> Result<Definition, Fault> {
    if previous
        .assets
        .iter()
        .any(|asset| asset.source.hub != previous.source.hub)
    {
        return Err(Fault::invalid("OCR update requires one model Hub"));
    }
    let mut repositories: Vec<Repository> = Vec::new();
    for source in
        std::iter::once(&previous.source).chain(previous.assets.iter().map(|asset| &asset.source))
    {
        if !repositories
            .iter()
            .any(|repository| repository.source.repository == source.repository)
        {
            let latest = Source {
                revision: "HEAD".into(),
                ..source.clone()
            };
            repositories.push(super::repository(http, &latest, cancel).await?);
        }
    }
    let mut assets = Vec::new();
    for previous in &previous.assets {
        let repository = repositories
            .iter()
            .find(|repository| repository.source.repository == previous.source.repository)
            .expect("all asset repositories were loaded");
        let file = required(repository, &previous.path)?;
        if !super::super::commit(&file.source.revision)
            || file.bytes == 0
            || file.bytes
                > if previous.name == "dictionary.txt" {
                    SMALL_BYTES as u64
                } else {
                    MAX_ASSET_BYTES
                }
        {
            return Err(Fault::invalid(
                "OCR update requires bounded immutable assets",
            ));
        }
        let name = previous.name.as_str();
        let asset = if name == "dictionary.txt" {
            small(http, file, name, cancel).await?.1
        } else {
            asset(http, file, name, cancel).await?
        };
        assets.push(asset);
    }
    let primary = &repositories[0];
    let definition = Definition {
        source: primary.source.clone(),
        name: previous.name.clone(),
        license: primary.license.clone(),
        assets,
        ..previous.clone()
    };
    definition.validate()?;
    Ok(definition)
}
