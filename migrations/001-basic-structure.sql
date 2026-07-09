CREATE TABLE invites (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMP NOT NULL,
    guild bigint NOT NULL,
    inviter bigint NOT NULL,
    max_age int NOT NULL,
    max_usages int NOT NULL,
    code TEXT NOT NULL
);

CREATE UNIQUE INDEX invite_code ON invites (code);

CREATE TABLE joined_member (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    last_join TIMESTAMP NOT NULL,
    user_id BIGINT NOT NULL,
    join_amount INT NOT NULL
);

CREATE UNIQUE INDEX join_amount_uniq ON joined_member (join_amount);
