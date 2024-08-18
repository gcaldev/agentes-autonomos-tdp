pub struct User {
    pub user: String,
    pub pass: String,
}

impl User {
    pub const fn new(user: String, pass: String) -> Self {
        User { user, pass }
    }
}
