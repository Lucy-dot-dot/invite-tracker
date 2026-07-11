/// Everything we need to know about the invite a member used to join.
pub struct UsedInvite {
    pub code: String,
    pub inviter_id: u64,
    #[allow(dead_code)]
    pub inviter_name: String,
    pub created_at: i64,
}