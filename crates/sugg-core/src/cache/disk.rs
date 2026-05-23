use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DiskCache {
    cache_dir: PathBuf,
}

impl DiskCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        self.cache_dir.join(format!("{:x}.cache", hasher.finish()))
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let path = self.cache_path(key);
        let text = fs::read_to_string(&path).ok()?;
        let (expire_str, content) = text.split_once('\n')?;
        let expire_at: u64 = expire_str.trim().parse().ok()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if now >= expire_at {
            return None;
        }
        Some(content.to_string())
    }

    pub fn delete(&self, key: &str) {
        let _ = fs::remove_file(self.cache_path(key));
    }

    pub fn set(&self, key: &str, val: &str, ttl_secs: u64) -> std::io::Result<()> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir)?;
        }
        let expire_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_secs()
            + ttl_secs;

        let data = format!("{}\n{}", expire_at, val);

        let tmp = tempfile::NamedTempFile::new_in(&self.cache_dir)?;
        fs::write(tmp.path(), data)?;
        tmp.persist(self.cache_path(key))
            .map_err(std::io::Error::other)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_set_and_get() {
        let dir = tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf());
        cache.set("k", "hello", 60).unwrap();
        assert_eq!(cache.get("k"), Some("hello".to_string()));
    }

    #[test]
    fn test_get_miss() {
        let dir = tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf());
        assert_eq!(cache.get("missing"), None);
    }

    #[test]
    fn test_delete() {
        let dir = tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf());
        cache.set("k", "v", 60).unwrap();
        cache.delete("k");
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn test_expired() {
        let dir = tempdir().unwrap();
        let cache = DiskCache::new(dir.path().to_path_buf());
        // ttl=0 秒，写入后立即过期
        cache.set("k", "v", 0).unwrap();
        assert_eq!(cache.get("k"), None);
    }
}
