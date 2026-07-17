CREATE TABLE IF NOT EXISTS af_review_taxonomy (
    taxonomy_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    parent_id UUID REFERENCES af_review_taxonomy(taxonomy_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('subject', 'topic', 'content')),
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    created_by BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (workspace_id, parent_id, kind, normalized_name)
);

CREATE UNIQUE INDEX IF NOT EXISTS af_review_taxonomy_root_unique
    ON af_review_taxonomy (workspace_id, kind, normalized_name)
    WHERE parent_id IS NULL;

CREATE TABLE IF NOT EXISTS af_flashcard (
    card_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    created_by BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    source_view_id UUID,
    source_block_id TEXT,
    card_type TEXT NOT NULL CHECK (card_type IN ('classic', 'multiple_choice', 'true_false')),
    front TEXT NOT NULL,
    back TEXT NOT NULL DEFAULT '',
    explanation TEXT NOT NULL DEFAULT '',
    choices JSONB NOT NULL DEFAULT '[]'::JSONB,
    correct_answer TEXT NOT NULL DEFAULT '',
    subject_id UUID REFERENCES af_review_taxonomy(taxonomy_id) ON DELETE SET NULL,
    topic_id UUID REFERENCES af_review_taxonomy(taxonomy_id) ON DELETE SET NULL,
    content_id UUID REFERENCES af_review_taxonomy(taxonomy_id) ON DELETE SET NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    suspended BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS af_flashcard_workspace_idx ON af_flashcard(workspace_id);
CREATE INDEX IF NOT EXISTS af_flashcard_taxonomy_idx ON af_flashcard(workspace_id, subject_id, topic_id, content_id);
CREATE INDEX IF NOT EXISTS af_flashcard_tags_idx ON af_flashcard USING GIN(tags);

CREATE TABLE IF NOT EXISTS af_flashcard_review_state (
    card_id UUID NOT NULL REFERENCES af_flashcard(card_id) ON DELETE CASCADE,
    uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'new' CHECK (state IN ('new', 'learning', 'review', 'relearning', 'suspended')),
    due_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    interval_seconds BIGINT NOT NULL DEFAULT 0 CHECK (interval_seconds >= 0),
    difficulty_score DOUBLE PRECISION NOT NULL DEFAULT 3.0,
    correct_count INTEGER NOT NULL DEFAULT 0,
    incorrect_count INTEGER NOT NULL DEFAULT 0,
    lapse_count INTEGER NOT NULL DEFAULT 0,
    consecutive_correct INTEGER NOT NULL DEFAULT 0,
    consecutive_incorrect INTEGER NOT NULL DEFAULT 0,
    last_reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (card_id, uid)
);

CREATE INDEX IF NOT EXISTS af_flashcard_review_due_idx ON af_flashcard_review_state(uid, due_at);

CREATE TABLE IF NOT EXISTS af_review_daily_queue (
    review_date DATE NOT NULL,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES af_flashcard(card_id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'scheduled' CHECK (source IN ('scheduled', 'overdue', 'new')),
    first_reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (review_date, workspace_id, uid, card_id)
);

CREATE TABLE IF NOT EXISTS af_review_daily_progress (
    review_date DATE NOT NULL,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    completion_awarded BOOLEAN NOT NULL DEFAULT FALSE,
    completion_awarded_at TIMESTAMPTZ,
    xp_earned INTEGER NOT NULL DEFAULT 0,
    reviews_completed INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (review_date, workspace_id, uid)
);

CREATE TABLE IF NOT EXISTS af_review_profile (
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    total_xp BIGINT NOT NULL DEFAULT 0,
    current_streak INTEGER NOT NULL DEFAULT 0,
    longest_streak INTEGER NOT NULL DEFAULT 0,
    last_completed_date DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (workspace_id, uid)
);

CREATE TABLE IF NOT EXISTS af_flashcard_review_log (
    review_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES af_flashcard(card_id) ON DELETE CASCADE,
    review_date DATE NOT NULL,
    selected_answer TEXT,
    correct BOOLEAN NOT NULL,
    difficulty SMALLINT NOT NULL CHECK (difficulty BETWEEN 1 AND 5),
    scheduled BOOLEAN NOT NULL,
    interval_before_seconds BIGINT NOT NULL,
    interval_after_seconds BIGINT NOT NULL,
    due_before TIMESTAMPTZ NOT NULL,
    due_after TIMESTAMPTZ NOT NULL,
    response_time_ms INTEGER,
    xp_awarded INTEGER NOT NULL DEFAULT 0,
    reviewed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS af_flashcard_review_log_user_date_idx
    ON af_flashcard_review_log(workspace_id, uid, review_date, reviewed_at);
CREATE INDEX IF NOT EXISTS af_flashcard_review_log_card_idx
    ON af_flashcard_review_log(card_id, uid, reviewed_at);
