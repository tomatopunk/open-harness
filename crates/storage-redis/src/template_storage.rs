use agent_ports::{PortError, PortResult, TaskTemplate, TemplateStoragePort};
use async_trait::async_trait;
use redis::AsyncCommands;

pub struct RedisTemplateStorage {
    client: redis::Client,
}

impl RedisTemplateStorage {
    pub fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        Ok(Self { client })
    }
}

#[async_trait]
impl TemplateStoragePort for RedisTemplateStorage {
    async fn get_all_templates(&self) -> PortResult<Vec<TaskTemplate>> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;

        // 扫描所有 template:* 键
        let keys: Vec<String> = redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg("template:*")
            .arg("COUNT")
            .arg("1000")
            .query_async(&mut conn)
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;

        let mut templates = Vec::new();
        for key in keys {
            let data: Option<String> =
                conn.get(&key).await.map_err(|e| PortError::Storage(e.to_string()))?;

            if let Some(json) = data {
                let template: TaskTemplate = serde_json::from_str(&json)
                    .map_err(|e| PortError::Storage(format!("decode template: {e}")))?;
                templates.push(template);
            }
        }

        Ok(templates)
    }

    async fn get_template(&self, id: &str) -> PortResult<Option<TaskTemplate>> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;

        let key = format!("template:{}", id);
        let data: Option<String> =
            conn.get(&key).await.map_err(|e| PortError::Storage(e.to_string()))?;

        match data {
            Some(json) => {
                let template: TaskTemplate = serde_json::from_str(&json)
                    .map_err(|e| PortError::Storage(format!("decode template: {e}")))?;
                Ok(Some(template))
            }
            None => Ok(None),
        }
    }

    async fn find_matching_template(
        &self,
        goal: &str,
        _threshold: f64,
    ) -> PortResult<Option<TaskTemplate>> {
        // Redis 不支持复杂的相似度匹配，获取所有模板并在内存中匹配
        let templates = self.get_all_templates().await?;

        // 简单的子串匹配
        for template in templates {
            if template.goal_pattern.contains(goal) {
                return Ok(Some(template));
            }
        }

        Ok(None)
    }

    async fn create_template(&self, template: &TaskTemplate) -> PortResult<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;

        let key = format!("template:{}", template.id);
        let json = serde_json::to_string(template)
            .map_err(|e| PortError::Storage(format!("encode template: {e}")))?;

        let _: () = conn.set(&key, json).await.map_err(|e| PortError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn update_template(&self, template: &TaskTemplate) -> PortResult<()> {
        self.create_template(template).await
    }

    async fn delete_template(&self, id: &str) -> PortResult<()> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;

        let key = format!("template:{}", id);
        let _: () = conn.del(&key).await.map_err(|e| PortError::Storage(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::SubtaskSpec;

    #[tokio::test]
    async fn test_redis_template_storage() {
        // 需要 Redis 服务器运行
        let url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let storage = RedisTemplateStorage::connect(&url).unwrap();

        let template = TaskTemplate {
            id: "test".to_string(),
            description: "Test".to_string(),
            goal_pattern: "test".to_string(),
            subtasks: vec![SubtaskSpec {
                goal: "Test task".to_string(),
                input: serde_json::Value::Null,
                budget_steps: 5,
            }],
            allow_extension: true,
        };

        storage.create_template(&template).await.unwrap();

        let retrieved = storage.get_template("test").await.unwrap();
        assert!(retrieved.is_some());

        storage.delete_template("test").await.unwrap();
    }
}
