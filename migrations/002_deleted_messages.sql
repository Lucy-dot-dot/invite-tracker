CREATE TABLE messages (
    id BIGINT PRIMARY KEY,
    user_id BIGINT,
    message TEXT,
    embeds TEXT,
    edits INT NOT NULL DEFAULT 0
);