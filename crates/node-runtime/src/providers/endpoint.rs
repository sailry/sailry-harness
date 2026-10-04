use super::*;

pub(crate) fn parse(value: &str) -> Result<Url, Fault> {
    let value = value.trim();
    if value.is_empty() || value.len() > 2048 {
        return Err(invalid("provider endpoint is empty or too long"));
    }
    let value = if value.contains("://") {
        value.to_owned()
    } else {
        if value.starts_with('/') {
            return Err(invalid("provider endpoint must include a host"));
        }
        format!("https://{value}")
    };
    let url = Url::parse(&value).map_err(|_| invalid("provider endpoint is invalid"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "provider endpoint must be an HTTP URL without credentials, query or fragment",
        ));
    }
    Ok(url)
}

pub(crate) fn anthropic(value: &str) -> Result<Url, Fault> {
    let mut url = parse(value)?;
    // Anthropic requests append /v1 themselves, including requests made by ADK.
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path).to_owned();
    url.set_path(&path);
    Ok(url)
}

pub(super) fn candidates(api: ModelApi, value: &str) -> Result<Vec<Url>, Fault> {
    if api == ModelApi::Anthropic {
        return Ok(vec![anthropic(value)?]);
    }
    let url = parse(value)?;
    let path = url.path().trim_end_matches('/');
    let versioned = path.rsplit('/').next().is_some_and(|part| {
        part.strip_prefix('v')
            .and_then(|suffix| suffix.as_bytes().first())
            .is_some_and(u8::is_ascii_digit)
    });
    let versions: &[&str] = if versioned {
        &[]
    } else {
        match api {
            ModelApi::ChatCompletions | ModelApi::Responses | ModelApi::DeepSeek => &["v1"],
            ModelApi::Gemini => &["v1beta", "v1"],
            _ => &[],
        }
    };
    let mut candidates = vec![url.clone()];
    for version in versions {
        let mut candidate = url.clone();
        candidate.set_path(&format!("{path}/{version}"));
        candidates.push(candidate);
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_bare_domains_to_https() {
        for (input, expected) in [
            ("example.test", "https://example.test/"),
            (" example.test:8443/api ", "https://example.test:8443/api"),
            ("http://localhost:8080", "http://localhost:8080/"),
        ] {
            assert_eq!(parse(input).unwrap().as_str(), expected);
        }
        for input in [
            "",
            "/v1",
            "//example.test",
            "user:password@example.test",
            "ftp://example.test",
        ] {
            assert!(parse(input).is_err());
        }
    }
}
