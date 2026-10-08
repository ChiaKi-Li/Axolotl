use crate::util::fetch::{DownloadRouteSource, ResourceClass};

const MODRINTH_MIRROR_HOST: &str = "mod.tianpao.top";

pub(crate) fn curseforge_candidate_urls(url: &str) -> Vec<String> {
    let candidates = [
        url.replace("-service.overwolf.wtf", ".forgecdn.net")
            .replace("://edge.", "://mediafilez.")
            .replace("://media.", "://mediafilez."),
        url.replace("://edge.", "://mediafilez.")
            .replace("://media.", "://mediafilez."),
        url.replace("-service.overwolf.wtf", ".forgecdn.net"),
        url.replace("://media.", "://edge."),
        url.to_string(),
    ];
    deduplicate(candidates.to_vec())
}

pub(crate) fn curseforge_download_urls(url: &str) -> Vec<String> {
    deduplicate(
        std::iter::once(url.to_string())
            .chain(curseforge_candidate_urls(url))
            .collect(),
    )
}

pub(crate) fn modrinth_resource_urls(
    urls: &[String],
    game_version: Option<&str>,
    loader: Option<&str>,
    api_latency: Option<std::time::Duration>,
) -> Vec<String> {
    let official_first =
        api_latency.is_some_and(|latency| latency.as_millis() < 4000);
    let mut output = Vec::new();
    for url in urls {
        let tracked = curseforge_candidate_urls(url);
        for candidate in tracked {
            if !candidate.to_ascii_lowercase().contains("cdn.modrinth.com/") {
                output.push(candidate.clone());
            }
            let mut tracked = candidate;
            tracked.push_str(if tracked.contains('?') { "&" } else { "?" });
            tracked.push_str("mr_download_reason=modpack");
            if let Some(game_version) = game_version {
                tracked.push_str("&mr_game_version=");
                tracked.push_str(game_version);
            }
            if let Some(loader) = loader {
                tracked.push_str("&mr_loader=");
                tracked.push_str(loader);
            }
            output.push(tracked);
        }
    }
    let output = deduplicate(output);
    let mut ordered = Vec::with_capacity(output.len() * 2);
    for url in output {
        if let Some(mirror) = modrinth_mirror_url(&url) {
            if official_first {
                ordered.push(url);
                ordered.push(mirror);
            } else {
                ordered.push(mirror);
                ordered.push(url);
            }
        } else {
            ordered.push(url);
        }
    }
    deduplicate(ordered)
}

pub(crate) fn modrinth_pack_urls(
    urls: &[String],
    api_latency: Option<std::time::Duration>,
) -> Vec<String> {
    let official_first =
        api_latency.is_some_and(|latency| latency.as_millis() < 4000);
    let mut ordered = Vec::new();
    for url in urls {
        if let Some(mirror) = modrinth_mirror_url(url) {
            if official_first {
                ordered.extend([url.clone(), mirror]);
            } else {
                ordered.extend([mirror, url.clone()]);
            }
        } else {
            ordered.push(url.clone());
        }
    }
    deduplicate(ordered)
}

pub(crate) fn exact_provider_route(
    url: String,
    resource: ResourceClass,
) -> crate::util::fetch::DownloadRoute {
    let host = url::Url::parse(&url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    let tianpao = host == MODRINTH_MIRROR_HOST;
    let curseforge_cdn =
        host == "forgecdn.net" || host.ends_with(".forgecdn.net");
    crate::util::fetch::DownloadRoute {
        url,
        source: if tianpao {
            DownloadRouteSource::Tianpao
        } else if curseforge_cdn {
            DownloadRouteSource::Alternate
        } else {
            DownloadRouteSource::Official
        },
        is_mirror: tianpao,
        allow_sensitive_headers: resource == ResourceClass::CurseForge
            && curseforge_cdn,
        supports_range: true,
        proxy: crate::util::fetch::ProxyPolicy::System,
    }
}

fn modrinth_mirror_url(url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("cdn.modrinth.com" | "cdn-alt.modrinth.com")
        )
        || !parsed.path().starts_with("/data/")
    {
        return None;
    }
    parsed.set_host(Some(MODRINTH_MIRROR_HOST)).ok()?;
    Some(parsed.into())
}

fn deduplicate(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curseforge_candidates_follow_script_transform_order() {
        assert_eq!(
            curseforge_candidate_urls(
                "https://edge.forgecdn.net/files/1/2/example.jar"
            ),
            [
                "https://mediafilez.forgecdn.net/files/1/2/example.jar",
                "https://edge.forgecdn.net/files/1/2/example.jar",
            ]
        );
    }

    #[test]
    fn curseforge_downloads_keep_the_original_url_before_script_variants() {
        assert_eq!(
            curseforge_download_urls(
                "https://edge.forgecdn.net/files/1/2/example.jar"
            ),
            [
                "https://edge.forgecdn.net/files/1/2/example.jar",
                "https://mediafilez.forgecdn.net/files/1/2/example.jar",
            ]
        );
    }

    #[test]
    fn modrinth_auto_source_order_uses_api_latency_and_keeps_tracking() {
        let urls = modrinth_resource_urls(
            &["https://cdn.modrinth.com/data/example.jar".to_string()],
            Some("1.20.1"),
            Some("fabric"),
            Some(std::time::Duration::from_millis(3999)),
        );
        assert!(urls[0].starts_with("https://cdn.modrinth.com/"));
        assert!(urls[1].starts_with("https://mod.tianpao.top/"));
        assert!(urls.iter().all(|url| {
            url.contains("mr_download_reason=modpack")
                && url.contains("mr_game_version=1.20.1")
                && url.contains("mr_loader=fabric")
        }));
    }

    #[test]
    fn missing_modrinth_api_latency_selects_mirror_first() {
        let urls = modrinth_resource_urls(
            &["https://cdn.modrinth.com/data/example.jar".to_string()],
            None,
            None,
            None,
        );
        assert!(urls[0].starts_with("https://mod.tianpao.top/"));
        assert!(urls[1].starts_with("https://cdn.modrinth.com/"));
    }

    #[test]
    fn modrinth_pack_urls_match_script_order_without_resource_tracking() {
        let urls = modrinth_pack_urls(
            &["https://cdn.modrinth.com/data/pack/version/pack.mrpack".into()],
            Some(std::time::Duration::from_millis(3999)),
        );

        assert!(urls[0].starts_with("https://cdn.modrinth.com/"));
        assert!(urls[1].starts_with("https://mod.tianpao.top/"));
        assert!(urls.iter().all(|url| !url.contains("mr_download_reason")));
    }

    #[test]
    fn exact_provider_routes_retain_system_dns_and_proxy() {
        let route = exact_provider_route(
            "https://cdn.modrinth.com/data/example.jar".to_string(),
            ResourceClass::Modpack,
        );

        assert_eq!(route.proxy, crate::util::fetch::ProxyPolicy::System);
        assert_eq!(route.url, "https://cdn.modrinth.com/data/example.jar");
        assert!(route.supports_range);
    }
}
