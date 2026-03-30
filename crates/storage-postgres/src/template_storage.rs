use agent_ports::{PortError, PortResult, TaskTemplate, TemplateStoragePort};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgPool;

pub struct PostgresTemplateStorage {
    pool: PgPool,
}

impl PostgresTemplateStorage {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        
        // 创建表（与 manage_task 保持一致的模式）
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS task_templates (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                goal_pattern TEXT NOT NULL,
                subtasks JSONB NOT NULL,
                allow_extension BOOLEAN NOT NULL DEFAULT true,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                version BIGINT NOT NULL DEFAULT 1
            );
            CREATE INDEX IF NOT EXISTS idx_task_templates_pattern ON task_templates USING GIN(goal_pattern gin_trgm_ops);
            "#,
        )
        .execute(&pool)
        .await?;
        
        Ok(Self { pool })
    }
}

#[async_trait]
impl TemplateStoragePort for PostgresTemplateStorage {
    async fn get_all_templates(&self) -> PortResult<Vec<TaskTemplate>> {
        let rows: Vec<(String, String, String, Value, bool)> = sqlx::query_as(
            r#"
            SELECT id, description, goal_pattern, subtasks, allow_extension
            FROM task_templates
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        let templates = rows
            .into_iter()
            .map(|(id, description, goal_pattern, subtasks, allow_extension)| {
                let subtasks = serde_json::from_value::<Vec<agent_ports::SubtaskSpec>>(subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension,
                })
            })
            .collect::<PortResult<Vec<_>>>()?;
        
        Ok(templates)
    }
    
    async fn get_template(&self, id: &str) -> PortResult<Option<TaskTemplate>> {
        let row: Option<(String, String, String, Value, bool)> = sqlx::query_as(
            r#"
            SELECT id, description, goal_pattern, subtasks, allow_extension
            FROM task_templates
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        match row {
            Some((id, description, goal_pattern, subtasks, allow_extension)) => {
                let subtasks = serde_json::from_value::<Vec<agent_ports::SubtaskSpec>>(subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(Some(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension,
                }))
            }
            None => Ok(None),
        }
    }
    
    async fn find_matching_template(&self, goal: &str, threshold: f64) -> PortResult<Option<TaskTemplate>> {
        // 使用 pg_trgm 进行相似度匹配
        let row: Option<(String, String, String, Value, bool)> = sqlx::query_as(
            r#"
            SELECT id, description, goal_pattern, subtasks, allow_extension
            FROM task_templates
            WHERE similarity(goal_pattern, $1) >= $2
            ORDER BY similarity(goal_pattern, $1) DESC
            LIMIT 1
            "#,
        )
        .bind(goal)
        .bind(threshold)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        match row {
            Some((id, description, goal_pattern, subtasks, allow_extension)) => {
                let subtasks = serde_json::from_value::<Vec<agent_ports::SubtaskSpec>>(subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(Some(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension,
                }))
            }
            None => Ok(None),
        }
    }
    
    async fn create_template(&self, template: &TaskTemplate) -> PortResult<()> {
        let subtasks_json = serde_json::to_value(&template.subtasks)
            .map_err(|e| PortError::Storage(format!("encode subtasks: {e}")))?;
        
        sqlx::query(
            r#"
            INSERT INTO task_templates (id, description, goal_pattern, subtasks, allow_extension)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(&template.id)
        .bind(&template.description)
        .bind(&template.goal_pattern)
        .bind(subtasks_json)
        .bind(template.allow_extension)
        .execute(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        Ok(())
    }
    
    async fn update_template(&self, template: &TaskTemplate) -> PortResult<()> {
        let subtasks_json = serde_json::to_value(&template.subtasks)
            .map_err(|e| PortError::Storage(format!("encode subtasks: {e}")))?;
        
        sqlx::query(
            r#"
            UPDATE task_templates
            SET description = $2,
                goal_pattern = $3,
                subtasks = $4,
                allow_extension = $5,
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(&template.id)
        .bind(&template.description)
        .bind(&template.goal_pattern)
        .bind(subtasks_json)
        .bind(template.allow_extension)
        .execute(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        Ok(())
    }
    
    async fn delete_template(&self, id: &str) -> PortResult<()> {
        sqlx::query("DELETE FROM task_templates WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| PortError::Storage(e.to_string()))?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_ports::SubtaskSpec;
    
    #[tokio::test]
    async fn test_template_crud() {
        // 注意：这个测试需要一个真实的 PostgreSQL 数据库
        // 可以使用 testcontainers 或本地数据库运行
        let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://localhost/harness_test".to_string()
        });
        
        let storage = PostgresTemplateStorage::connect(&url).await.unwrap();
        
        // 创建模板
        let template = TaskTemplate {
            id: "test_template".to_string(),
            description: "Test template".to_string(),
            goal_pattern: "测试 |test".to_string(),
            subtasks: vec![
                SubtaskSpec {
                    goal: "Test task 1".to_string(),
                    input: serde_json::Value::Null,
                    budget_steps: 5,
                },
            ],
            allow_extension: true,
        };
        
        storage.create_template(&template).await.unwrap();
        
        // 获取模板
        let retrieved = storage.get_template("test_template").await.unwrap();
        assert!(retrieved.is_some());
        
        // 清理
        storage.delete_template("test_template").await.unwrap();
    }
}
