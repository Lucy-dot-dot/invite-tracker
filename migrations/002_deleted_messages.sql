CREATE TABLE messages (
    id BIGINT PRIMARY KEY DEFAULT,
    user BIGINT,
    message TEXT,
    embeds TEXT,
    edits INT NOT NULL DEFAULT 0
);