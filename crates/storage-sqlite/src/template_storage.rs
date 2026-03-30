use agent_ports::{PortError, PortResult, TaskTemplate, TemplateStoragePort};
use async_trait::async_trait;
use sqlx::SqlitePool;

pub struct SqliteTemplateStorage {
    pool: SqlitePool,
}

impl SqliteTemplateStorage {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(url).await?;
        
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS task_templates (
                id TEXT PRIMARY KEY,
                description TEXT NOT NULL,
                goal_pattern TEXT NOT NULL,
                subtasks TEXT NOT NULL,
                allow_extension INTEGER NOT NULL DEFAULT 1,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                version INTEGER NOT NULL DEFAULT 1
            );
            "#,
        )
        .execute(&pool)
        .await?;
        
        Ok(Self { pool })
    }
}

#[async_trait]
impl TemplateStoragePort for SqliteTemplateStorage {
    async fn get_all_templates(&self) -> PortResult<Vec<TaskTemplate>> {
        let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
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
                let subtasks = serde_json::from_str::<Vec<agent_ports::SubtaskSpec>>(&subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension: allow_extension != 0,
                })
            })
            .collect::<PortResult<Vec<_>>>()?;
        
        Ok(templates)
    }
    
    async fn get_template(&self, id: &str) -> PortResult<Option<TaskTemplate>> {
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
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
                let subtasks = serde_json::from_str::<Vec<agent_ports::SubtaskSpec>>(&subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(Some(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension: allow_extension != 0,
                }))
            }
            None => Ok(None),
        }
    }
    
    async fn find_matching_template(&self, goal: &str, _threshold: f64) -> PortResult<Option<TaskTemplate>> {
        // SQLite 不支持 trigram 相似度，使用简单的 LIKE 匹配
        let row: Option<(String, String, String, String, i64)> = sqlx::query_as(
            r#"
            SELECT id, description, goal_pattern, subtasks, allow_extension
            FROM task_templates
            WHERE goal_pattern LIKE $1
            ORDER BY id
            LIMIT 1
            "#,
        )
        .bind(format!("%{}%", goal))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        match row {
            Some((id, description, goal_pattern, subtasks, allow_extension)) => {
                let subtasks = serde_json::from_str::<Vec<agent_ports::SubtaskSpec>>(&subtasks)
                    .map_err(|e| PortError::Storage(format!("decode subtasks: {e}")))?;
                
                Ok(Some(TaskTemplate {
                    id,
                    description,
                    goal_pattern,
                    subtasks,
                    allow_extension: allow_extension != 0,
                }))
            }
            None => Ok(None),
        }
    }
    
    async fn create_template(&self, template: &TaskTemplate) -> PortResult<()> {
        let subtasks_json = serde_json::to_string(&template.subtasks)
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
        .bind(&subtasks_json)
        .bind(if template.allow_extension { 1 } else { 0 })
        .execute(&self.pool)
        .await
        .map_err(|e| PortError::Storage(e.to_string()))?;
        
        Ok(())
    }
    
    async fn update_template(&self, template: &TaskTemplate) -> PortResult<()> {
        let subtasks_json = serde_json::to_string(&template.subtasks)
            .map_err(|e| PortError::Storage(format!("encode subtasks: {e}")))?;
        
        sqlx::query(
            r#"
            UPDATE task_templates
            SET description = $2,
                goal_pattern = $3,
                subtasks = $4,
                allow_extension = $5,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $1
            "#,
        )
        .bind(&template.id)
        .bind(&template.description)
        .bind(&template.goal_pattern)
        .bind(&subtasks_json)
        .bind(if template.allow_extension { 1 } else { 0 })
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
