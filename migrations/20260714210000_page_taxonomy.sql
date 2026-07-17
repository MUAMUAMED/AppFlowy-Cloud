ALTER TABLE af_review_taxonomy
  ADD COLUMN IF NOT EXISTS source_view_id UUID,
  ADD COLUMN IF NOT EXISTS auto_managed BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX IF NOT EXISTS af_review_taxonomy_source_view_idx
  ON af_review_taxonomy(workspace_id, source_view_id)
  WHERE source_view_id IS NOT NULL;
