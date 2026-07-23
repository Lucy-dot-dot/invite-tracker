CREATE TABLE messages (
    id BIGINT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    message TEXT NOT NULL,
    attachments TEXT,
    edits INT NOT NULL DEFAULT 0
);