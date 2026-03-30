-- Migration: Create task_templates table
-- Description: Stores reusable task decomposition templates
-- Created: 2024-01-01

-- Enable pg_trgm extension for similarity matching (PostgreSQL only)
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Create task_templates table
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

-- Create index for similarity matching
CREATE INDEX IF NOT EXISTS idx_task_templates_pattern ON task_templates USING GIN(goal_pattern gin_trgm_ops);

-- Create index for sorting
CREATE INDEX IF NOT EXISTS idx_task_templates_created ON task_templates(created_at DESC);

-- Add comment
COMMENT ON TABLE task_templates IS 'Task decomposition templates for reusable workflows';
COMMENT ON COLUMN task_templates.id IS 'Unique template identifier';
COMMENT ON COLUMN task_templates.description IS 'Human-readable description';
COMMENT ON COLUMN task_templates.goal_pattern IS 'Pattern for matching goals (supports regex)';
COMMENT ON COLUMN task_templates.subtasks IS 'List of subtasks in JSON format';
COMMENT ON COLUMN task_templates.allow_extension IS 'Whether LLM can extend the template';
COMMENT ON COLUMN task_templates.version IS 'Optimistic locking version';
