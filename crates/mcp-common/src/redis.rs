/// Redis cache wrapper with graceful degradation.
///
/// All operations return `Option<T>` — on any Redis error, the operation logs a warning
/// and returns `None`. Callers fall through to compute from source. The system is fully
/// functional without Redis.
use redis::AsyncCommands;
use tracing::warn;

pub struct RedisCache {
    client: Option<redis::Client>,
}

impl RedisCache {
    /// Attempt to connect to Redis. If the URL is `None` or connection fails,
    /// returns a `RedisCache` that always degrades gracefully (no-ops).
    pub fn new(url: Option<&str>) -> Self {
        let client = url.and_then(|u| {
            redis::Client::open(u)
                .inspect_err(|e| warn!(error = %e, url = u, "failed to create redis client, cache disabled"))
                .ok()
        });
        Self { client }
    }

    /// Test the connection by sending a PING. Returns `true` if Redis is reachable.
    pub async fn is_available(&self) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        match client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                let result: Result<String, _> = redis::cmd("PING").query_async(&mut conn).await;
                result.is_ok()
            }
            Err(_) => false,
        }
    }

    /// Get a value from Redis. Returns `None` if Redis is unavailable or the key doesn't exist.
    pub async fn get(&self, key: &str) -> Option<String> {
        let client = self.client.as_ref()?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .inspect_err(|e| warn!(error = %e, "redis connection failed"))
            .ok()?;
        let value: Option<String> = conn
            .get(key)
            .await
            .inspect_err(|e| warn!(error = %e, key, "redis GET failed"))
            .ok()?;
        value
    }

    /// Set a value in Redis with no expiry. Returns `true` if successful.
    pub async fn set(&self, key: &str, value: &str) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let Ok(mut conn) = client
            .get_multiplexed_async_connection()
            .await
            .inspect_err(|e| warn!(error = %e, "redis connection failed"))
        else {
            return false;
        };
        conn.set::<_, _, ()>(key, value)
            .await
            .inspect_err(|e| warn!(error = %e, key, "redis SET failed"))
            .is_ok()
    }

    /// Set a value in Redis with a TTL in seconds. Returns `true` if successful.
    pub async fn set_with_ttl(&self, key: &str, value: &str, ttl_secs: u64) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let Ok(mut conn) = client
            .get_multiplexed_async_connection()
            .await
            .inspect_err(|e| warn!(error = %e, "redis connection failed"))
        else {
            return false;
        };
        conn.set_ex::<_, _, ()>(key, value, ttl_secs)
            .await
            .inspect_err(|e| warn!(error = %e, key, "redis SETEX failed"))
            .is_ok()
    }

    /// Delete a specific key. Returns `true` if successful.
    pub async fn delete(&self, key: &str) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let Ok(mut conn) = client
            .get_multiplexed_async_connection()
            .await
            .inspect_err(|e| warn!(error = %e, "redis connection failed"))
        else {
            return false;
        };
        conn.del::<_, ()>(key)
            .await
            .inspect_err(|e| warn!(error = %e, key, "redis DEL failed"))
            .is_ok()
    }

    /// Delete all keys matching a prefix using SCAN (not KEYS, which blocks).
    /// Pattern is constructed as `{prefix}*`.
    pub async fn delete_by_prefix(&self, prefix: &str) -> bool {
        let Some(client) = &self.client else {
            return false;
        };
        let Ok(mut conn) = client
            .get_multiplexed_async_connection()
            .await
            .inspect_err(|e| warn!(error = %e, "redis connection failed"))
        else {
            return false;
        };

        let pattern = format!("{prefix}*");
        let mut cursor: u64 = 0;
        loop {
            let (next_cursor, keys): (u64, Vec<String>) =
                match redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(&pattern)
                    .arg("COUNT")
                    .arg(100)
                    .query_async(&mut conn)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        warn!(error = %e, pattern, "redis SCAN failed");
                        return false;
                    }
                };

            if !keys.is_empty() {
                if let Err(e) = conn.del::<_, ()>(&keys).await {
                    warn!(error = %e, "redis batch DEL failed during prefix delete");
                    return false;
                }
            }

            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }
        true
    }
}
