CREATE TABLE invites (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at BIGINT NOT NULL,
    guild BIGINT NOT NULL,
    inviter BIGINT NOT NULL,
    max_age INT NOT NULL,
    max_usages INT NOT NULL,
    code TEXT NOT NULL,
    uses BIGINT NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX invite_code ON invites (code);

CREATE TABLE joined_member (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    last_join BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    join_amount INT NOT NULL
);

CREATE UNIQUE INDEX joined_member_user_uniq ON joined_member (user_id);