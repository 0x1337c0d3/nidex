use chrono::DateTime;
use chrono::Utc;
use codex_protocol::openai_models::ModelInfo;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tracing::error;

/// Manages loading and saving of models cache to disk.
#[derive(Debug)]
pub(crate) struct ModelsCacheManager {
    cache_path: PathBuf,
    cache_ttl: Duration,
}

impl ModelsCacheManager {
    /// Create a new cache manager with the given path and TTL.
    pub(crate) fn new(cache_path: PathBuf, cache_ttl: Duration) -> Self {
        Self {
            cache_path,
            cache_ttl,
        }
    }

    /// Attempt to load a fresh cache entry. Returns `None` if the cache doesn't exist or is stale.
    pub(crate) async fn load_fresh(
        &self,
        expected_version: &str,
        provider_base_url: Option<&str>,
    ) -> Option<ModelsCache> {
        tracing::debug!("load_fresh called with version={}, provider_url={:?}", expected_version, provider_base_url);
        let cache = match self.load().await {
            Ok(cache) => cache,
            Err(err) => {
                error!("failed to load models cache: {err}");
                return None;
            }
        };
        let cache = match cache {
            Some(c) => c,
            None => {
                tracing::debug!("no cache file found");
                return None;
            }
        };
        tracing::debug!("cache loaded: version={:?}, provider_url={:?}", cache.client_version, cache.provider_base_url);
        if cache.client_version.as_deref() != Some(expected_version) {
            tracing::debug!("version mismatch: cache={:?}, expected={}", cache.client_version, expected_version);
            return None;
        }
        if !cache.is_fresh(self.cache_ttl) {
            return None;
        }
        // Verify the cache was fetched for the same provider base URL
        if let Some(expected_url) = provider_base_url {
            let cache_url = cache.provider_base_url.as_deref().unwrap_or("");
            // Normalize both URLs for comparison (remove trailing slashes)
            let normalized_cache = if cache_url.ends_with('/') {
                &cache_url[..cache_url.len() - 1]
            } else {
                cache_url
            };
            let normalized_expected = if expected_url.ends_with('/') {
                &expected_url[..expected_url.len() - 1]
            } else {
                expected_url
            };
            tracing::debug!(
                expected_url = %normalized_expected,
                cache_url = %normalized_cache,
                "checking provider base URL match"
            );
            if normalized_cache != normalized_expected {
                tracing::debug!("provider URL mismatch, cache invalid");
                return None;
            }
        } else if cache.provider_base_url.is_some() {
            // Cache has a provider URL but we don't expect one
            tracing::debug!("cache has provider URL but none expected, cache invalid");
            return None;
        }
        Some(cache)
    }

    /// Persist the cache to disk, creating parent directories as needed.
    pub(crate) async fn persist_cache(
        &self,
        models: &[ModelInfo],
        etag: Option<String>,
        client_version: String,
        provider_base_url: Option<String>,
    ) {
        let cache = ModelsCache {
            fetched_at: Utc::now(),
            etag,
            client_version: Some(client_version),
            models: models.to_vec(),
            provider_base_url,
        };
        if let Err(err) = self.save_internal(&cache).await {
            error!("failed to write models cache: {err}");
        }
    }

    /// Renew the cache TTL by updating the fetched_at timestamp to now.
    pub(crate) async fn renew_cache_ttl(&self) -> io::Result<()> {
        let mut cache = match self.load().await? {
            Some(cache) => cache,
            None => return Err(io::Error::new(ErrorKind::NotFound, "cache not found")),
        };
        cache.fetched_at = Utc::now();
        self.save_internal(&cache).await
    }

    async fn load(&self) -> io::Result<Option<ModelsCache>> {
        match fs::read(&self.cache_path).await {
            Ok(contents) => {
                let cache = serde_json::from_slice(&contents)
                    .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
                Ok(Some(cache))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn save_internal(&self, cache: &ModelsCache) -> io::Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_vec_pretty(cache)
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err.to_string()))?;
        fs::write(&self.cache_path, json).await
    }

    #[cfg(test)]
    /// Set the cache TTL.
    pub(crate) fn set_ttl(&mut self, ttl: Duration) {
        self.cache_ttl = ttl;
    }

    #[cfg(test)]
    /// Manipulate cache file for testing. Allows setting a custom fetched_at timestamp.
    pub(crate) async fn manipulate_cache_for_test<F>(&self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut DateTime<Utc>),
    {
        let mut cache = match self.load().await? {
            Some(cache) => cache,
            None => return Err(io::Error::new(ErrorKind::NotFound, "cache not found")),
        };
        f(&mut cache.fetched_at);
        self.save_internal(&cache).await
    }

    #[cfg(test)]
    /// Mutate the full cache contents for testing.
    pub(crate) async fn mutate_cache_for_test<F>(&self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut ModelsCache),
    {
        let mut cache = match self.load().await? {
            Some(cache) => cache,
            None => return Err(io::Error::new(ErrorKind::NotFound, "cache not found")),
        };
        f(&mut cache);
        self.save_internal(&cache).await
    }
}

/// Serialized snapshot of models and metadata cached on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ModelsCache {
    pub(crate) fetched_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) client_version: Option<String>,
    pub(crate) models: Vec<ModelInfo>,
    /// Base URL of the provider that fetched these models (for cache key discrimination).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) provider_base_url: Option<String>,
}

impl ModelsCache {
    /// Returns `true` when the cache entry has not exceeded the configured TTL.
    fn is_fresh(&self, ttl: Duration) -> bool {
        if ttl.is_zero() {
            return false;
        }
        let Ok(ttl_duration) = chrono::Duration::from_std(ttl) else {
            return false;
        };
        let age = Utc::now().signed_duration_since(self.fetched_at);
        age <= ttl_duration
    }
}
