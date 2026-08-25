-- NOT IMPLEMENTED YET.  This schema is reserved for future, explicit
-- read-only GitHub commit snapshots. The GitHub repository URL and credentials
-- must never enter the database or the browser response.
CREATE TABLE IF NOT EXISTS card_github_commits (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    sha TEXT NOT NULL,
    short_sha TEXT NOT NULL,
    message TEXT NOT NULL,
    author_name TEXT NOT NULL,
    committed_at TIMESTAMPTZ,
    additions INTEGER NOT NULL DEFAULT 0 CHECK (additions >= 0),
    deletions INTEGER NOT NULL DEFAULT 0 CHECK (deletions >= 0),
    file_count INTEGER NOT NULL DEFAULT 0 CHECK (file_count >= 0),
    files_truncated BOOLEAN NOT NULL DEFAULT FALSE,
    attached_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (card_id, sha)
);

CREATE INDEX IF NOT EXISTS card_github_commits_card_created_idx
    ON card_github_commits(card_id, created_at DESC);

CREATE TABLE IF NOT EXISTS card_github_commit_files (
    id UUID PRIMARY KEY,
    github_commit_id UUID NOT NULL REFERENCES card_github_commits(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    status TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0 CHECK (additions >= 0),
    deletions INTEGER NOT NULL DEFAULT 0 CHECK (deletions >= 0),
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0)
);

CREATE INDEX IF NOT EXISTS card_github_commit_files_commit_position_idx
    ON card_github_commit_files(github_commit_id, position);
