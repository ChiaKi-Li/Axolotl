//! Per-authority physical connection admission; transfer weights use the shared download semaphore.

use crate::util::fetch::{DownloadRoute, ProxyPolicy};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

const MAX_CONNECTIONS_PER_AUTHORITY: usize = 32;
const MAX_PROVIDER_CONNECTIONS_PER_AUTHORITY: usize = 128;
const MAX_PHYSICAL_CONNECTIONS: usize = 256;
static PHYSICAL_CONNECTIONS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(MAX_PHYSICAL_CONNECTIONS)));

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct AuthorityKey {
    authority: String,
    proxy: ProxyPolicy,
    connection_limit: usize,
}

static AUTHORITY_BUDGETS: LazyLock<
    Mutex<HashMap<AuthorityKey, Arc<Semaphore>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct NativeBudgetPermit {
    _authority: Option<OwnedSemaphorePermit>,
    _global: OwnedSemaphorePermit,
}

fn budget(
    route: &DownloadRoute,
    connection_limit: usize,
) -> Option<Arc<Semaphore>> {
    let authority = crate::util::fetch::url_authority(&route.url)?;
    let key = AuthorityKey {
        authority: super::proxy_context::authority_key(&authority, route.proxy),
        proxy: route.proxy,
        connection_limit,
    };
    let mut budgets = AUTHORITY_BUDGETS.lock();
    if budgets.len() >= 256 {
        budgets.retain(|_, budget| Arc::strong_count(budget) > 1);
    }
    Some(
        budgets
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(connection_limit)))
            .clone(),
    )
}

pub(crate) async fn acquire(
    route: &DownloadRoute,
) -> Result<NativeBudgetPermit, tokio::sync::AcquireError> {
    acquire_with_global_limit(
        route,
        &PHYSICAL_CONNECTIONS,
        MAX_CONNECTIONS_PER_AUTHORITY,
    )
    .await
}

pub(crate) async fn acquire_provider(
    route: &DownloadRoute,
) -> Result<NativeBudgetPermit, tokio::sync::AcquireError> {
    acquire_with_global_limit(
        route,
        &PHYSICAL_CONNECTIONS,
        MAX_PROVIDER_CONNECTIONS_PER_AUTHORITY,
    )
    .await
}

async fn acquire_with_global(
    route: &DownloadRoute,
    global: &Arc<Semaphore>,
) -> Result<NativeBudgetPermit, tokio::sync::AcquireError> {
    acquire_with_global_limit(route, global, MAX_CONNECTIONS_PER_AUTHORITY)
        .await
}

async fn acquire_with_global_limit(
    route: &DownloadRoute,
    global: &Arc<Semaphore>,
    connection_limit: usize,
) -> Result<NativeBudgetPermit, tokio::sync::AcquireError> {
    let authority = match budget(route, connection_limit) {
        Some(budget) => {
            if budget.available_permits() == 0 {
                super::h2_pool::evict_idle_connections(Some(
                    &super::proxy_context::authority_key(
                        &crate::util::fetch::url_authority(&route.url).unwrap(),
                        route.proxy,
                    ),
                ))
                .await;
            }
            Some(budget.acquire_owned().await?)
        }
        None => None,
    };
    if global.available_permits() == 0 {
        super::h2_pool::evict_idle_connections(None).await;
    }
    let global = global.clone().acquire_owned().await?;
    Ok(NativeBudgetPermit {
        _authority: authority,
        _global: global,
    })
}

pub(crate) async fn acquire_many(
    route: &DownloadRoute,
    count: usize,
) -> Result<Vec<NativeBudgetPermit>, tokio::sync::AcquireError> {
    acquire_many_with_limit(route, count, MAX_CONNECTIONS_PER_AUTHORITY).await
}

pub(crate) async fn acquire_many_provider(
    route: &DownloadRoute,
    count: usize,
) -> Result<Vec<NativeBudgetPermit>, tokio::sync::AcquireError> {
    acquire_many_with_limit(
        route,
        count,
        MAX_PROVIDER_CONNECTIONS_PER_AUTHORITY,
    )
    .await
}

async fn acquire_many_with_limit(
    route: &DownloadRoute,
    count: usize,
    connection_limit: usize,
) -> Result<Vec<NativeBudgetPermit>, tokio::sync::AcquireError> {
    let count = count.min(connection_limit);
    let mut authority = match budget(route, connection_limit) {
        Some(budget) => {
            if budget.available_permits() < count {
                super::h2_pool::evict_idle_connections(Some(
                    &super::proxy_context::authority_key(
                        &crate::util::fetch::url_authority(&route.url).unwrap(),
                        route.proxy,
                    ),
                ))
                .await;
            }
            Some(
                budget
                    .acquire_many_owned(count.min(connection_limit) as u32)
                    .await?,
            )
        }
        None => None,
    };
    if PHYSICAL_CONNECTIONS.available_permits() < count {
        super::h2_pool::evict_idle_connections(None).await;
    }
    let mut global = PHYSICAL_CONNECTIONS
        .clone()
        .acquire_many_owned(count as u32)
        .await?;
    Ok((0..count.min(connection_limit))
        .map(|_| NativeBudgetPermit {
            _global: global
                .split(1)
                .expect("global batch contains enough permits"),
            _authority: authority.as_mut().map(|permit| {
                permit
                    .split(1)
                    .expect("authority batch contains enough permits")
            }),
        })
        .collect())
}

pub(crate) fn try_acquire(
    route: &DownloadRoute,
) -> Result<NativeBudgetPermit, TryAcquireError> {
    try_acquire_with_limit(route, MAX_CONNECTIONS_PER_AUTHORITY)
}

pub(crate) fn try_acquire_provider(
    route: &DownloadRoute,
) -> Result<NativeBudgetPermit, TryAcquireError> {
    try_acquire_with_limit(route, MAX_PROVIDER_CONNECTIONS_PER_AUTHORITY)
}

fn try_acquire_with_limit(
    route: &DownloadRoute,
    connection_limit: usize,
) -> Result<NativeBudgetPermit, TryAcquireError> {
    Ok(NativeBudgetPermit {
        _global: PHYSICAL_CONNECTIONS.clone().try_acquire_owned()?,
        _authority: match budget(route, connection_limit) {
            Some(budget) => Some(budget.try_acquire_owned()?),
            None => None,
        },
    })
}

pub(crate) fn available(route: &DownloadRoute) -> usize {
    available_with_limit(route, MAX_CONNECTIONS_PER_AUTHORITY)
}

fn available_with_limit(
    route: &DownloadRoute,
    connection_limit: usize,
) -> usize {
    budget(route, connection_limit)
        .map(|budget| budget.available_permits())
        .unwrap_or(connection_limit)
        .min(PHYSICAL_CONNECTIONS.available_permits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::fetch::DownloadRouteSource;

    #[tokio::test]
    async fn global_physical_budget_bounds_multiple_authorities_and_releases_after_drop()
     {
        let global = Arc::new(Semaphore::new(2));
        let mut first = route();
        first.url = "https://global-first.invalid/file".into();
        let mut second = first.clone();
        second.url = "https://global-second.invalid/file".into();
        let a = acquire_with_global(&first, &global).await.unwrap();
        let b = acquire_with_global(&second, &global).await.unwrap();
        let pending = acquire_with_global(&first, &global);
        tokio::pin!(pending);
        assert!(futures::poll!(pending.as_mut()).is_pending());
        drop(a);
        let c = pending.await.unwrap();
        drop((b, c));
        assert_eq!(global.available_permits(), 2);
    }

    fn route() -> DownloadRoute {
        DownloadRoute {
            url: "https://budget.example/file".to_string(),
            source: DownloadRouteSource::Official,
            is_mirror: false,
            allow_sensitive_headers: true,
            supports_range: true,
            proxy: ProxyPolicy::Direct,
        }
    }

    #[tokio::test]
    async fn authority_budget_is_bounded() {
        let route = route();
        let mut permits = Vec::new();
        for _ in 0..MAX_CONNECTIONS_PER_AUTHORITY {
            permits.push(acquire(&route).await.unwrap());
        }
        assert!(matches!(
            try_acquire(&route),
            Err(TryAcquireError::NoPermits)
        ));
        drop(permits);
        assert!(try_acquire(&route).is_ok());
    }

    #[tokio::test]
    async fn provider_authority_budget_matches_launcher_concurrency() {
        let mut route = route();
        route.url = "https://provider-budget.example/file".into();
        let global = Arc::new(Semaphore::new(64));
        let mut permits = Vec::new();
        for _ in 0..64 {
            permits.push(
                acquire_with_global_limit(
                    &route,
                    &global,
                    MAX_PROVIDER_CONNECTIONS_PER_AUTHORITY,
                )
                .await
                .unwrap(),
            );
        }
        assert_eq!(global.available_permits(), 0);
        drop(permits);
        assert_eq!(global.available_permits(), 64);
    }
}
